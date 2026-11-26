use super::{
    input::{host_shortcut, HostShortcut},
    vnc::{Connection, Input},
};
use std::{
    collections::{HashMap, HashSet},
    ffi::CStr,
    ptr,
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};
use x11_dl::xlib;

pub(crate) fn prefer_x11_when_available() {
    if !std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty()) {
        return;
    }
    let Ok(xlib) = xlib::Xlib::open() else {
        return;
    };
    if unsafe { (xlib.XInitThreads)() } == 0 {
        return;
    }
    let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
    if display.is_null() {
        return;
    }
    unsafe { (xlib.XCloseDisplay)(display) };
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::remove_var("WAYLAND_SOCKET");
}

pub(crate) struct NativeKeyboardCapture {
    stop: mpsc::Sender<()>,
    ready: Option<mpsc::Receiver<Result<(), String>>>,
    worker: Option<JoinHandle<()>>,
}

impl NativeKeyboardCapture {
    pub(crate) fn start(
        connection: Arc<Mutex<Connection>>,
        ctx: egui::Context,
        expected_title: String,
        shortcuts: mpsc::Sender<HostShortcut>,
        host_key_enabled: bool,
    ) -> Self {
        let (stop, stopped) = mpsc::channel();
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            run(
                connection,
                ctx,
                expected_title,
                shortcuts,
                host_key_enabled,
                stopped,
                ready,
            )
        });
        Self {
            stop,
            ready: Some(started),
            worker: Some(worker),
        }
    }

    pub(crate) fn poll_ready(&mut self) -> Option<Result<(), String>> {
        let result = self.ready.as_ref()?.try_recv();
        match result {
            Ok(result) => {
                self.ready = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.ready = None;
                Some(Err("X11 keyboard capture stopped unexpectedly.".into()))
            }
        }
    }
}

impl Drop for NativeKeyboardCapture {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(
    connection: Arc<Mutex<Connection>>,
    ctx: egui::Context,
    expected_title: String,
    shortcuts: mpsc::Sender<HostShortcut>,
    host_key_enabled: bool,
    stopped: mpsc::Receiver<()>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let xlib = match xlib::Xlib::open() {
        Ok(xlib) => xlib,
        Err(error) => {
            let _ = ready.send(Err(format!("Could not load X11: {error}")));
            return;
        }
    };
    let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
    if display.is_null() {
        let _ = ready.send(Err(
            "No X11 display is available (Wayland sessions do not support this grab).".into(),
        ));
        return;
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut failure = "The remote viewer did not receive keyboard focus.";
    let window = loop {
        if stopped.try_recv().is_ok() {
            unsafe { (xlib.XCloseDisplay)(display) };
            return;
        }
        let mut focused = 0;
        let mut revert = 0;
        unsafe { (xlib.XGetInputFocus)(display, &mut focused, &mut revert) };
        if focused != 0 {
            let mut title_pointer = ptr::null_mut();
            unsafe { (xlib.XFetchName)(display, focused, &mut title_pointer) };
            let focused_title = if !title_pointer.is_null() {
                let title = unsafe { CStr::from_ptr(title_pointer) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { (xlib.XFree)(title_pointer.cast()) };
                title
            } else {
                String::new()
            };
            if focused_title == expected_title {
                let result = unsafe {
                    (xlib.XGrabKeyboard)(
                        display,
                        focused,
                        xlib::False,
                        xlib::GrabModeAsync,
                        xlib::GrabModeAsync,
                        xlib::CurrentTime,
                    )
                };
                if result == xlib::GrabSuccess {
                    break focused;
                }
                failure = "Another application owns the X11 keyboard grab.";
            }
        }
        if std::time::Instant::now() >= deadline {
            unsafe { (xlib.XCloseDisplay)(display) };
            let _ = ready.send(Err(failure.into()));
            return;
        }
        thread::sleep(Duration::from_millis(30));
    };
    unsafe { (xlib.XSync)(display, xlib::False) };
    if ready.send(Ok(())).is_err() {
        unsafe {
            (xlib.XUngrabKeyboard)(display, xlib::CurrentTime);
            (xlib.XCloseDisplay)(display);
        }
        return;
    }

    let mut held = HashMap::new();
    let mut host_key: Option<(u32, HostKeyState)> = None;
    let mut suppressed = HashSet::new();
    let mut grabbed = true;
    loop {
        if stopped.try_recv().is_ok() {
            break;
        }
        let mut focused_window = 0;
        let mut revert = 0;
        unsafe { (xlib.XGetInputFocus)(display, &mut focused_window, &mut revert) };
        if grabbed && focused_window != window {
            release_held(&connection, &mut held);
            host_key = None;
            suppressed.clear();
            unsafe { (xlib.XUngrabKeyboard)(display, xlib::CurrentTime) };
            grabbed = false;
            ctx.request_repaint();
        } else if !grabbed && focused_window == window {
            grabbed = unsafe {
                (xlib.XGrabKeyboard)(
                    display,
                    window,
                    xlib::False,
                    xlib::GrabModeAsync,
                    xlib::GrabModeAsync,
                    xlib::CurrentTime,
                ) == xlib::GrabSuccess
            };
            if grabbed {
                unsafe { (xlib.XSync)(display, xlib::False) };
                ctx.request_repaint();
            }
        }
        if !grabbed {
            thread::sleep(Duration::from_millis(60));
            continue;
        }
        while unsafe { (xlib.XPending)(display) } > 0 {
            let mut event = xlib::XEvent { pad: [0; 24] };
            unsafe { (xlib.XNextEvent)(display, &mut event) };
            let event_type = unsafe { event.type_ };
            if event_type != xlib::KeyPress && event_type != xlib::KeyRelease {
                continue;
            }
            let pressed = event_type == xlib::KeyPress;
            let mut key = unsafe { event.key };
            if pressed {
                let level = if key.state & xlib::ShiftMask != 0 {
                    1
                } else {
                    0
                };
                let keysym = unsafe { (xlib.XLookupKeysym)(&mut key, level) } as u32;
                if keysym == 0 {
                    continue;
                }
                if host_key_enabled && keysym == 0xffe4 && host_key.is_none() {
                    host_key = Some((key.keycode, HostKeyState::Pending));
                    continue;
                }
                if let Some((host_keycode, HostKeyState::Pending)) = host_key {
                    if let Some(shortcut) = host_shortcut(keysym) {
                        let _ = shortcuts.send(shortcut);
                        host_key = Some((host_keycode, HostKeyState::Shortcut));
                        suppressed.insert(key.keycode);
                        ctx.request_repaint();
                        continue;
                    }
                    connection.lock().unwrap().send_input(Input::Key {
                        keysym: 0xffe4,
                        physical_key: None,
                        pressed: true,
                    });
                    held.insert(host_keycode, 0xffe4);
                    host_key = Some((host_keycode, HostKeyState::Forwarded));
                }
                connection.lock().unwrap().send_input(Input::Key {
                    keysym,
                    physical_key: None,
                    pressed: true,
                });
                held.insert(key.keycode, keysym);
            } else {
                if suppressed.remove(&key.keycode) {
                    continue;
                }
                if let Some((host_keycode, state)) = host_key {
                    if host_keycode == key.keycode {
                        match state {
                            HostKeyState::Pending => {
                                let connection = connection.lock().unwrap();
                                connection.send_input(Input::Key {
                                    keysym: 0xffe4,
                                    physical_key: None,
                                    pressed: true,
                                });
                                connection.send_input(Input::Key {
                                    keysym: 0xffe4,
                                    physical_key: None,
                                    pressed: false,
                                });
                            }
                            HostKeyState::Forwarded => {
                                if let Some(keysym) = held.remove(&key.keycode) {
                                    connection.lock().unwrap().send_input(Input::Key {
                                        keysym,
                                        physical_key: None,
                                        pressed: false,
                                    });
                                }
                            }
                            HostKeyState::Shortcut => {}
                        }
                        host_key = None;
                        continue;
                    }
                }
                if let Some(keysym) = held.remove(&key.keycode) {
                    connection.lock().unwrap().send_input(Input::Key {
                        keysym,
                        physical_key: None,
                        pressed: false,
                    });
                }
            }
        }
        thread::sleep(Duration::from_millis(4));
    }

    release_held(&connection, &mut held);
    if grabbed {
        unsafe { (xlib.XUngrabKeyboard)(display, xlib::CurrentTime) };
    }
    unsafe {
        (xlib.XSync)(display, xlib::False);
        (xlib.XCloseDisplay)(display);
    }
    ctx.request_repaint();
}

#[derive(Clone, Copy)]
enum HostKeyState {
    Pending,
    Forwarded,
    Shortcut,
}

fn release_held(connection: &Arc<Mutex<Connection>>, held: &mut HashMap<u32, u32>) {
    let connection = connection.lock().unwrap();
    for keysym in held.drain().map(|(_, keysym)| keysym) {
        connection.send_input(Input::Key {
            keysym,
            physical_key: None,
            pressed: false,
        });
    }
}
