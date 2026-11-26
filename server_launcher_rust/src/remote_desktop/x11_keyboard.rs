use super::vnc::{Connection, Input};
use std::{
    collections::HashMap,
    ffi::CStr,
    ptr,
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};
use x11_dl::xlib;

pub(crate) struct NativeKeyboardCapture {
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl NativeKeyboardCapture {
    pub(crate) fn start(
        connection: Arc<Mutex<Connection>>,
        ctx: egui::Context,
        expected_title: String,
    ) -> Result<Self, String> {
        let (stop, stopped) = mpsc::channel();
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || run(connection, ctx, expected_title, stopped, ready));
        match started.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                let _ = stop.send(());
                let _ = worker.join();
                Err(format!("Could not start X11 keyboard capture: {error}"))
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

    let mut window = 0;
    let mut revert = 0;
    unsafe { (xlib.XGetInputFocus)(display, &mut window, &mut revert) };
    if window == 0 {
        unsafe { (xlib.XCloseDisplay)(display) };
        let _ = ready.send(Err("The remote viewer does not have keyboard focus.".into()));
        return;
    }
    let mut title_pointer = ptr::null_mut();
    let has_title = unsafe { (xlib.XFetchName)(display, window, &mut title_pointer) } != 0;
    let focused_title = if has_title && !title_pointer.is_null() {
        let title = unsafe { CStr::from_ptr(title_pointer) }
            .to_string_lossy()
            .into_owned();
        unsafe { (xlib.XFree)(title_pointer.cast()) };
        title
    } else {
        String::new()
    };
    if focused_title != expected_title {
        unsafe { (xlib.XCloseDisplay)(display) };
        let _ = ready.send(Err(
            "The focused window is not this remote viewer; keyboard capture was not started."
                .into(),
        ));
        return;
    }
    let result = unsafe {
        (xlib.XGrabKeyboard)(
            display,
            window,
            xlib::False,
            xlib::GrabModeAsync,
            xlib::GrabModeAsync,
            xlib::CurrentTime,
        )
    };
    if result != xlib::GrabSuccess {
        unsafe { (xlib.XCloseDisplay)(display) };
        let _ = ready.send(Err("Another application owns the X11 keyboard grab.".into()));
        return;
    }
    unsafe { (xlib.XSync)(display, xlib::False) };
    if ready.send(Ok(())).is_err() {
        unsafe {
            (xlib.XUngrabKeyboard)(display, xlib::CurrentTime);
            (xlib.XCloseDisplay)(display);
        }
        return;
    }

    let mut held = HashMap::new();
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
                connection.lock().unwrap().send_input(Input::Key {
                    keysym,
                    physical_key: None,
                    pressed: true,
                });
                held.insert(key.keycode, keysym);
            } else {
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
