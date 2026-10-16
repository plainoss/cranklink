//! Shared file facade. Browser writes are persisted by the host before sync ACK.
#[cfg(not(target_arch = "wasm32"))]
pub use std::fs::{read, File, OpenOptions};

#[cfg(not(target_arch = "wasm32"))]
pub fn read_bounded(
    path: impl AsRef<std::path::Path>,
    max_bytes: usize,
) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let f = File::open(path)?;
    let mut data = Vec::new();
    f.take(max_bytes as u64 + 1).read_to_end(&mut data)?;
    if data.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file exceeds size limit",
        ));
    }
    Ok(data)
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use serde_json::json;
    use std::{
        cell::RefCell,
        io::{self, Read, Write},
        path::Path,
    };
    fn call(op: &str, value: serde_json::Value) -> io::Result<Vec<u8>> {
        crate::wasm::rpc(
            op,
            &serde_json::to_vec(&value).map_err(io::Error::other)?,
            30_000,
        )
    }
    fn path_string(path: impl AsRef<Path>) -> io::Result<String> {
        path.as_ref()
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"))
    }
    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        call("fs.read", json!({"path":path_string(path)?}))
    }
    pub fn read_bounded(path: impl AsRef<Path>, max_bytes: usize) -> io::Result<Vec<u8>> {
        let data = call(
            "fs.read",
            json!({"path":path_string(path)?,"max_bytes":max_bytes}),
        )?;
        if data.len() > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file exceeds size limit",
            ));
        }
        Ok(data)
    }
    pub struct File {
        path: String,
        read_data: Vec<u8>,
        read_cursor: usize,
        writable: bool,
        pending: RefCell<Vec<u8>>,
    }
    impl File {
        pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
            let path = path_string(path)?;
            let read_data = read(&path)?;
            Ok(Self {
                path,
                read_data,
                read_cursor: 0,
                writable: false,
                pending: RefCell::new(Vec::new()),
            })
        }
        fn persist_pending(&self) -> io::Result<()> {
            let mut pending = self.pending.borrow_mut();
            if !pending.is_empty() {
                call("fs.append", json!({"path":self.path,"data":&*pending}))?;
                pending.clear();
            }
            Ok(())
        }
        pub fn sync_all(&self) -> io::Result<()> {
            self.persist_pending()?;
            call("fs.sync", json!({"path":self.path}))?;
            Ok(())
        }
    }
    impl Read for File {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = out.len().min(self.read_data.len() - self.read_cursor);
            out[..n].copy_from_slice(&self.read_data[self.read_cursor..self.read_cursor + n]);
            self.read_cursor += n;
            Ok(n)
        }
    }
    impl Write for File {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            if !self.writable {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "file was not opened for writing",
                ));
            }
            self.pending.borrow_mut().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.persist_pending()
        }
    }
    #[derive(Default)]
    pub struct OpenOptions {
        create: bool,
        create_new: bool,
        append: bool,
        write: bool,
        truncate: bool,
    }
    impl OpenOptions {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn create(&mut self, value: bool) -> &mut Self {
            self.create = value;
            self
        }
        pub fn create_new(&mut self, value: bool) -> &mut Self {
            self.create_new = value;
            self
        }
        pub fn append(&mut self, value: bool) -> &mut Self {
            self.append = value;
            self
        }
        pub fn write(&mut self, value: bool) -> &mut Self {
            self.write = value;
            self
        }
        pub fn truncate(&mut self, value: bool) -> &mut Self {
            self.truncate = value;
            self
        }
        pub fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
            let path = path_string(path)?;
            if !self.write && !self.append {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "write or append access is required",
                ));
            }
            // The CLI only creates exclusive files or appends journals/traces.
            // Reject random overwrite access rather than pretending append is seek.
            if !self.append && !self.create_new && !self.truncate {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "browser files require append, truncate, or create_new",
                ));
            }
            call(
                "fs.create",
                json!({"path":path,"create":self.create,"create_new":self.create_new,
                "append":self.append,"write":self.write,"truncate":self.truncate}),
            )?;
            Ok(File {
                path,
                read_data: Vec::new(),
                read_cursor: 0,
                writable: true,
                pending: RefCell::new(Vec::new()),
            })
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub use browser::{read, read_bounded, File, OpenOptions};
