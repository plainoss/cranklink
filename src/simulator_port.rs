//! Native connection to the optional offline firmware simulator.
//! Simulator sockets are explicit; automatic device discovery never selects one.
use crate::platform::Port;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

pub(crate) struct SimulatorPort(UnixStream);

impl SimulatorPort {
    pub(crate) fn open(path: &str) -> io::Result<Self> {
        if !Path::new(path).is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sim: requires an absolute Unix socket path",
            ));
        }
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;
        Ok(Self(stream))
    }
}
impl Read for SimulatorPort {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let read = self.0.read(bytes)?;
        if read == 0 && !bytes.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "firmware simulator closed the session",
            ));
        }
        Ok(read)
    }
}
impl Write for SimulatorPort {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
impl Port for SimulatorPort {
    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.0.set_read_timeout(Some(timeout))?;
        self.0.set_write_timeout(Some(timeout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_paths_are_rejected_before_connecting() {
        assert!(
            matches!(SimulatorPort::open("device.sock"), Err(e) if e.kind() == io::ErrorKind::InvalidInput)
        );
    }
    #[test]
    fn disconnect_is_an_error_and_does_not_spin_until_command_timeout() {
        let (local, remote) = UnixStream::pair().unwrap();
        drop(remote);
        let mut port = SimulatorPort(local);
        let error = port.read(&mut [0; 128]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
}
