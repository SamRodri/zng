use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::{env, fmt};

use ipmpsc::{Receiver, Sender, SharedRingBuffer};
use webrender::api::units::LayoutSize;
use webrender::api::{BuiltDisplayListDescriptor, PipelineId};

use crate::{message::*, CHANNEL_VAR, VERSION};

/// View process controller.
pub struct App {
    process: Child,
    request_sender: Sender,
    response_receiver: Receiver,
    event_receiver: Receiver,
    windows: Vec<Window>,
    devices: Vec<Device>,
}

impl App {
    /// Start the view process as an instance of the [`current_exe`].
    ///
    /// [`current_exe`]: std::env::current_exe
    pub fn start(request: StartRequest) -> Self {
        Self::start_with(std::env::current_exe().unwrap(), request)
    }

    /// Start with a custom view process.
    pub fn start_with(view_process_exe: PathBuf, request: StartRequest) -> Self {
        let channel_dir = loop {
            let temp_dir = env::temp_dir().join(uuid::Uuid::new_v4().to_simple().to_string());
            match std::fs::create_dir(&temp_dir) {
                Ok(_) => break temp_dir,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("failed to create channel directory: {}", e),
            }
        };

        let response_receiver = Receiver::new(
            SharedRingBuffer::create(channel_dir.join("response").display().to_string().as_str(), MAX_RESPONSE_SIZE)
                .expect("response channel creation failed"),
        );
        let event_receiver = Receiver::new(
            SharedRingBuffer::create(channel_dir.join("event").display().to_string().as_str(), MAX_RESPONSE_SIZE)
                .expect("event channel creation failed"),
        );
        let request_sender = Sender::new(
            SharedRingBuffer::create(channel_dir.join("request").display().to_string().as_str(), MAX_REQUEST_SIZE)
                .expect("request channel creation failed"),
        );

        // create process and spawn it
        let process = Command::new(view_process_exe)
            .env(CHANNEL_VAR, channel_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("renderer view process failed to spawn");

        let app = App {
            process,
            request_sender,
            response_receiver,
            event_receiver,
            windows: vec![],
            devices: vec![],
        };

        match app.request(Request::ProtocolVersion) {
            Response::ProtocolVersion(v) => {
                if v != VERSION {
                    panic!(
                        "`zero-ui-wr {}` is not supported by this controller, ensure /
                    the `zero-ui-wr` crate is the same version in all involved executables",
                        v
                    );
                }
            }
            _ => panic!("view process did not start correctly"),
        }

        match app.request(Request::Start(request)) {
            Response::Started => app,
            _ => panic!("view process did not start correctly"),
        }
    }

    fn request(&self, request: Request) -> Response {
        self.request_sender.send(&request).unwrap();
        self.response_receiver.recv().unwrap()
    }

    /// Open a window.
    pub fn open_window(&mut self, request: OpenWindowRequest) -> u32 {
        let mut window = Window {
            id: 0,
            title: request.title.clone(),
            pos: request.pos,
            size: request.size,
            visible: request.visible,
            frame: request.frame.clone(),
        };
        match self.request(Request::OpenWindow(request)) {
            Response::WindowOpened(id) => {
                window.id = id;
                self.windows.push(window);
                id
            }
            _ => panic!("view process did not respond correctly"),
        }
    }

    /// Set the window title.
    pub fn set_title(&mut self, window: WinId, title: String) -> Result<(), WindowNotFound> {
        match self.windows.iter_mut().position(|w| w.id == window) {
            Some(i) => match self.request(Request::SetWindowTitle(window, title.clone())) {
                Response::WindowTitleChanged(id) if id == window => {
                    self.windows[i].title = title;
                    Ok(())
                }
                Response::WindowNotFound(id) if id == window => {
                    self.windows.remove(i);
                    Err(WindowNotFound(id))
                }
                _ => panic!("view process did not respond correctly"),
            },
            None => Err(WindowNotFound(window)),
        }
    }

    /// Set the window position.
    pub fn set_position(&mut self, window: WinId, pos: (i32, i32)) -> Result<(), WindowNotFound> {
        match self.windows.iter_mut().position(|w| w.id == window) {
            Some(i) => match self.request(Request::SetWindowPosition(window, pos)) {
                Response::WindowMoved(id, pos) if id == window => {
                    self.windows[i].pos = pos;
                    Ok(())
                }
                Response::WindowNotFound(id) if id == window => {
                    self.windows.remove(i);
                    Err(WindowNotFound(id))
                }
                _ => panic!("view process did not respond correctly"),
            },
            None => Err(WindowNotFound(window)),
        }
    }

    /// Set the window size.
    pub fn set_size(&mut self, window: WinId, size: (u32, u32)) -> Result<(), WindowNotFound> {
        match self.windows.iter_mut().position(|w| w.id == window) {
            Some(i) => match self.request(Request::SetWindowSize(window, size)) {
                Response::WindowResized(id, size) if id == window => {
                    self.windows[i].size = size;
                    Ok(())
                }
                Response::WindowNotFound(id) if id == window => {
                    self.windows.remove(i);
                    Err(WindowNotFound(id))
                }
                _ => panic!("view process did not respond correctly"),
            },
            None => Err(WindowNotFound(window)),
        }
    }

    /// Set the window visibility.
    pub fn set_visible(&mut self, window: WinId, visible: bool) -> Result<(), WindowNotFound> {
        match self.windows.iter_mut().position(|w| w.id == window) {
            Some(i) => match self.request(Request::SetWindowVisible(window, visible)) {
                Response::WindowVisibilityChanged(id, visible) if id == window => {
                    self.windows[i].visible = visible;
                    Ok(())
                }
                Response::WindowNotFound(id) if id == window => {
                    self.windows.remove(i);
                    Err(WindowNotFound(id))
                }
                _ => panic!("view process did not respond correctly"),
            },
            None => Err(WindowNotFound(window)),
        }
    }

    /// Close the window.
    pub fn close_window(&mut self, window: WinId) -> Result<(), WindowNotFound> {
        match self.windows.iter().position(|w| w.id == window) {
            Some(i) => {
                self.windows.remove(i);
            }
            None => return Err(WindowNotFound(window)),
        }

        match self.request(Request::CloseWindow(window)) {
            Response::WindowClosed(id) if id == window => Ok(()),
            Response::WindowNotFound(id) if id == window => Err(WindowNotFound(id)),
            _ => panic!("view process did not respond correctly"),
        }
    }

    /// Gracefully shutdown the view process, returns when the process is closed.
    pub fn shutdown(mut self) {
        self.request_sender.send(&Request::Shutdown).unwrap();
        self.process.wait().unwrap();
    }
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}

struct Window {
    id: WinId,
    title: String,
    pos: (i32, i32),
    size: (u32, u32),
    visible: bool,
    frame: (PipelineId, LayoutSize, (Vec<u8>, BuiltDisplayListDescriptor)),
}

struct Device {
    id: DevId,
}

/// Error when a window ID is not opened in an [`App`].
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct WindowNotFound(pub WinId);
impl fmt::Display for WindowNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "window `{}` not found", self.0)
    }
}
impl std::error::Error for WindowNotFound {}
