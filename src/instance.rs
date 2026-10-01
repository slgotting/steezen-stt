use crate::engine::Shared;
use std::io::{Read, Write};
use std::sync::Arc;

#[cfg(unix)]
mod ipc {
    pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};

    fn path() -> std::path::PathBuf {
        dirs::runtime_dir().unwrap_or_else(std::env::temp_dir).join(format!("{}.sock", crate::settings::APP_NAME))
    }

    pub fn connect() -> std::io::Result<Stream> {
        Stream::connect(path())
    }

    pub fn bind() -> std::io::Result<Listener> {
        let _ = std::fs::remove_file(path());
        Listener::bind(path())
    }
}

#[cfg(windows)]
mod ipc {
    pub use std::net::{TcpListener as Listener, TcpStream as Stream};

    const ADDR: &str = "127.0.0.1:47613";

    pub fn connect() -> std::io::Result<Stream> {
        Stream::connect(ADDR)
    }

    pub fn bind() -> std::io::Result<Listener> {
        Listener::bind(ADDR)
    }
}

pub fn signal_existing() -> bool {
    ipc::connect().and_then(|mut s| s.write_all(b"show")).is_ok()
}

pub fn listen(shared: Arc<Shared>) {
    let Ok(listener) = ipc::bind() else { return };
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 4];
            if stream.read_exact(&mut buf).is_ok() && &buf == b"show" {
                crate::tray::show_window(&shared);
            }
        }
    });
}
