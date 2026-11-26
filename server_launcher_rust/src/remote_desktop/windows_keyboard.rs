use super::{
    profile::DesktopProtocol,
    vnc::{Connection, Input, InputHandle},
};
use std::{
    collections::HashMap,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, LPARAM, LRESULT, WPARAM},
    System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW,
        GetWindowThreadProcessId, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG,
        PM_REMOVE, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
    },
};

pub(crate) struct NativeKeyboardCapture {
    stop: Arc<AtomicBool>,
    thread_id: Arc<std::sync::atomic::AtomicU32>,
    ready: Option<mpsc::Receiver<Result<(), String>>>,
    worker: Option<JoinHandle<()>>,
    state: Option<Arc<Mutex<HookState>>>,
}

struct HookState {
    input: InputHandle,
    protocol: DesktopProtocol,
    process_id: u32,
    held: HashMap<u32, Input>,
    focused: bool,
}

static CAPTURE: OnceLock<Mutex<Option<Arc<Mutex<HookState>>>>> = OnceLock::new();

impl NativeKeyboardCapture {
    pub(crate) fn start(connection: Arc<Mutex<Connection>>, protocol: DesktopProtocol) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread_id = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let worker_thread_id = thread_id.clone();
        let (ready, started) = mpsc::sync_channel(1);
        let state = connection.lock().unwrap().input_handle().map(|input| {
            Arc::new(Mutex::new(HookState {
                input,
                protocol,
                process_id: unsafe { GetCurrentProcessId() },
                held: HashMap::new(),
                focused: true,
            }))
        });
        let worker_state = state.clone();
        let worker = thread::spawn(move || run(worker_state, worker_stop, worker_thread_id, ready));
        Self {
            stop,
            thread_id,
            ready: Some(started),
            worker: Some(worker),
            state,
        }
    }

    pub(crate) fn set_focused(&self, focused: bool) {
        if let Some(state) = &self.state {
            let mut state = state.lock().unwrap();
            if state.focused && !focused {
                release_held(&mut state);
            }
            state.focused = focused;
        }
    }

    pub(crate) fn poll_ready(&mut self) -> Option<Result<(), String>> {
        match self.ready.as_ref()?.try_recv() {
            Ok(result) => {
                self.ready = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.ready = None;
                Some(Err("Windows keyboard capture stopped unexpectedly.".into()))
            }
        }
    }
}

impl Drop for NativeKeyboardCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let thread_id = self.thread_id.load(Ordering::Acquire);
        if thread_id != 0 {
            unsafe { PostThreadMessageW(thread_id, WM_QUIT, 0, 0) };
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(
    state: Option<Arc<Mutex<HookState>>>,
    stop: Arc<AtomicBool>,
    thread_id: Arc<std::sync::atomic::AtomicU32>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let Some(state) = state else {
        let _ = ready.send(Err("The remote keyboard channel is not ready yet.".into()));
        return;
    };
    let global = CAPTURE.get_or_init(|| Mutex::new(None));
    {
        let mut current = global.lock().unwrap();
        if current.is_some() {
            let _ = ready.send(Err("Keyboard capture is already active.".into()));
            return;
        }
        *current = Some(state.clone());
    }
    let mut message = MSG::default();
    unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) };
    thread_id.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
    if stop.load(Ordering::Acquire) {
        global.lock().unwrap().take();
        thread_id.store(0, Ordering::Release);
        return;
    }
    let hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), ptr::null_mut(), 0) };
    if hook.is_null() {
        thread_id.store(0, Ordering::Release);
        global.lock().unwrap().take();
        let _ = ready.send(Err(format!(
            "Windows refused keyboard capture (error {}).",
            unsafe { GetLastError() }
        )));
        return;
    }
    if ready.send(Ok(())).is_err() {
        unsafe { UnhookWindowsHookEx(hook) };
        global.lock().unwrap().take();
        thread_id.store(0, Ordering::Release);
        return;
    }

    while !stop.load(Ordering::Acquire) {
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    unsafe { UnhookWindowsHookEx(hook) };
    release_held(&mut state.lock().unwrap());
    thread_id.store(0, Ordering::Release);
    let mut current = global.lock().unwrap();
    if current
        .as_ref()
        .is_some_and(|active| Arc::ptr_eq(active, &state))
    {
        current.take();
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code < HC_ACTION as i32 {
        return CallNextHookEx(ptr::null_mut(), code, message, data);
    }
    let Some(global) = CAPTURE.get() else {
        return CallNextHookEx(ptr::null_mut(), code, message, data);
    };
    let Ok(capture) = global.lock() else {
        return CallNextHookEx(ptr::null_mut(), code, message, data);
    };
    let Some(state) = capture.as_ref().cloned() else {
        return CallNextHookEx(ptr::null_mut(), code, message, data);
    };
    let Ok(mut state) = state.lock() else {
        return CallNextHookEx(ptr::null_mut(), code, message, data);
    };
    let foreground = foreground_process_id();
    if foreground != state.process_id {
        if state.focused {
            release_held(&mut state);
            state.focused = false;
        }
        drop(state);
        return CallNextHookEx(ptr::null_mut(), code, message, data);
    }
    if !state.focused {
        drop(state);
        return CallNextHookEx(ptr::null_mut(), code, message, data);
    }
    let pressed = match message as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => true,
        WM_KEYUP | WM_SYSKEYUP => false,
        _ => {
            drop(state);
            return CallNextHookEx(ptr::null_mut(), code, message, data);
        }
    };
    let key = &*(data as *const KBDLLHOOKSTRUCT);
    let extended = key.flags & LLKHF_EXTENDED != 0;
    let identity = (key.scanCode & 0xff) | if extended { 0x100 } else { 0 };
    if pressed {
        let event = match state.protocol {
            DesktopProtocol::Rdp if key.scanCode != 0 => Input::RdpKey {
                scan_code: key.scanCode as u8,
                extended,
                pressed: true,
            },
            DesktopProtocol::Vnc => match virtual_key_keysym(key.vkCode, key.scanCode, extended) {
                Some(keysym) => Input::Key {
                    keysym,
                    physical_key: None,
                    pressed: true,
                },
                None => {
                    drop(state);
                    return CallNextHookEx(ptr::null_mut(), code, message, data);
                }
            },
            _ => {
                drop(state);
                return CallNextHookEx(ptr::null_mut(), code, message, data);
            }
        };
        state.input.send(event.clone());
        state.held.insert(identity, event);
    } else if let Some(event) = state.held.remove(&identity) {
        state.input.send(released(event));
    } else {
        drop(state);
        return CallNextHookEx(ptr::null_mut(), code, message, data);
    }
    1
}

fn foreground_process_id() -> u32 {
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return 0;
    }
    let mut process_id = 0;
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    process_id
}

fn release_held(state: &mut HookState) {
    for (_, event) in state.held.drain() {
        state.input.send(released(event));
    }
}

fn released(event: Input) -> Input {
    match event {
        Input::Key {
            keysym,
            physical_key,
            ..
        } => Input::Key {
            keysym,
            physical_key,
            pressed: false,
        },
        Input::RdpKey {
            scan_code,
            extended,
            ..
        } => Input::RdpKey {
            scan_code,
            extended,
            pressed: false,
        },
        other => other,
    }
}

fn virtual_key_keysym(vk: u32, scan_code: u32, extended: bool) -> Option<u32> {
    Some(match vk {
        0x08 => 0xff08,
        0x09 => 0xff09,
        0x0d if extended => 0xff8d,
        0x0d => 0xff0d,
        0x10 if scan_code == 0x36 => 0xffe2,
        0x10 => 0xffe1,
        0x11 if extended => 0xffe4,
        0x11 => 0xffe3,
        0x12 if extended => 0xffea,
        0x12 => 0xffe9,
        0x13 => 0xff13,
        0x14 => 0xffe5,
        0x1b => 0xff1b,
        0x20 => 0x20,
        0x21 => 0xff55,
        0x22 => 0xff56,
        0x23 => 0xff57,
        0x24 => 0xff50,
        0x25 => 0xff51,
        0x26 => 0xff52,
        0x27 => 0xff53,
        0x28 => 0xff54,
        0x2c => 0xff61,
        0x2d => 0xff63,
        0x2e => 0xffff,
        0x30..=0x39 => vk,
        0x41..=0x5a => vk + 0x20,
        0x5b => 0xffeb,
        0x5c => 0xffec,
        0x5d => 0xff67,
        0x60..=0x69 => 0xffb0 + vk - 0x60,
        0x6a => 0xffaa,
        0x6b => 0xffab,
        0x6d => 0xffad,
        0x6e => 0xffae,
        0x6f => 0xffaf,
        0x70..=0x87 => 0xffbd + vk - 0x6f,
        0x90 => 0xff7f,
        0x91 => 0xff14,
        0xa0 => 0xffe1,
        0xa1 => 0xffe2,
        0xa2 => 0xffe3,
        0xa3 => 0xffe4,
        0xa4 => 0xffe9,
        0xa5 => 0xffea,
        0xba => b';' as u32,
        0xbb => b'=' as u32,
        0xbc => b',' as u32,
        0xbd => b'-' as u32,
        0xbe => b'.' as u32,
        0xbf => b'/' as u32,
        0xc0 => b'`' as u32,
        0xdb => b'[' as u32,
        0xdc | 0xe2 => b'\\' as u32,
        0xdd => b']' as u32,
        0xde => b'\'' as u32,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_windows_keys_to_vnc_keysyms() {
        assert_eq!(virtual_key_keysym(0x41, 0x1e, false), Some(b'a' as u32));
        assert_eq!(virtual_key_keysym(0x09, 0x0f, false), Some(0xff09));
        assert_eq!(virtual_key_keysym(0x5b, 0x5b, true), Some(0xffeb));
        assert_eq!(virtual_key_keysym(0x25, 0x4b, true), Some(0xff51));
        assert_eq!(virtual_key_keysym(0x10, 0x36, false), Some(0xffe2));
    }
}
