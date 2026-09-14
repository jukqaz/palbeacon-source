use std::{fmt, net::SocketAddr};

use pal_rest::ConnectedStreamVerifier;
use thiserror::Error;

pub struct LiveServerIdentity {
    endpoint: SocketAddr,
    process_id: u32,
    creation_time_100ns: u64,
    executable_sha256: [u8; 32],
    #[cfg(windows)]
    _process: std::os::windows::io::OwnedHandle,
    #[cfg(windows)]
    _image: std::fs::File,
}

impl LiveServerIdentity {
    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    pub const fn creation_time_100ns(&self) -> u64 {
        self.creation_time_100ns
    }

    pub const fn executable_sha256(&self) -> [u8; 32] {
        self.executable_sha256
    }

    pub fn revalidate(&self, rest_base_url: &str) -> Result<(), LiveProcessError> {
        let endpoint = parse_loopback_endpoint(rest_base_url)?;
        if endpoint != self.endpoint {
            return Err(LiveProcessError::EndpointMismatch);
        }
        platform::revalidate(endpoint, self)
    }

    fn verify_connected_stream(
        &self,
        local_address: SocketAddr,
        peer_address: SocketAddr,
    ) -> Result<(), LiveProcessError> {
        if peer_address != self.endpoint
            || !local_address.ip().is_loopback()
            || local_address.port() == 0
        {
            return Err(LiveProcessError::EndpointMismatch);
        }
        platform::verify_connected_stream(local_address, peer_address, self)
    }
}

impl ConnectedStreamVerifier for LiveServerIdentity {
    fn verify(&self, local_address: SocketAddr, peer_address: SocketAddr) -> bool {
        self.verify_connected_stream(local_address, peer_address)
            .is_ok()
    }
}

impl fmt::Debug for LiveServerIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveServerIdentity")
            .field("endpoint", &self.endpoint)
            .field("process_id", &self.process_id)
            .field("creation_time_100ns", &self.creation_time_100ns)
            .field("executable_sha256", &"[REDACTED]")
            .finish()
    }
}

pub fn inspect_live_server(
    rest_base_url: &str,
    process_id: u32,
) -> Result<LiveServerIdentity, LiveProcessError> {
    if process_id == 0 {
        return Err(LiveProcessError::InvalidProcess);
    }
    let endpoint = parse_loopback_endpoint(rest_base_url)?;
    platform::inspect(endpoint, process_id)
}

fn parse_loopback_endpoint(rest_base_url: &str) -> Result<SocketAddr, LiveProcessError> {
    let authority_and_path = rest_base_url
        .strip_prefix("http://")
        .ok_or(LiveProcessError::RemoteEndpoint)?;
    let authority = authority_and_path
        .split_once('/')
        .map_or(authority_and_path, |(authority, _)| authority);
    let endpoint = authority
        .parse::<SocketAddr>()
        .map_err(|_| LiveProcessError::RemoteEndpoint)?;
    if !endpoint.ip().is_loopback() || endpoint.port() == 0 {
        return Err(LiveProcessError::RemoteEndpoint);
    }
    Ok(endpoint)
}

#[cfg(windows)]
mod platform {
    use std::{
        ffi::OsString,
        fs::{File, OpenOptions},
        io::Read,
        mem,
        net::{IpAddr, SocketAddr},
        os::windows::{
            ffi::OsStringExt,
            fs::{MetadataExt, OpenOptionsExt},
            io::{FromRawHandle, OwnedHandle},
        },
        path::PathBuf,
        ptr,
    };

    use sha2::{Digest, Sha256};
    use windows_sys::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, FILETIME, NO_ERROR, STILL_ACTIVE},
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCP_STATE_ESTAB, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
            TCP_TABLE_OWNER_PID_CONNECTIONS, TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::{AF_INET, AF_INET6},
        Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        },
        System::Threading::{
            GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
    };

    use super::{LiveProcessError, LiveServerIdentity};

    pub(super) fn inspect(
        endpoint: SocketAddr,
        process_id: u32,
    ) -> Result<LiveServerIdentity, LiveProcessError> {
        let owner = listener_owner(endpoint)?;
        if owner != process_id {
            return Err(LiveProcessError::ListenerOwnerMismatch);
        }

        // SAFETY: the PID is an inert integer and only query-only access is requested.
        let raw_process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if raw_process.is_null() {
            return Err(LiveProcessError::InvalidProcess);
        }
        // SAFETY: OpenProcess returned a newly owned, non-null kernel handle.
        let process = unsafe { OwnedHandle::from_raw_handle(raw_process) };
        let creation_time_100ns = process_creation_time(raw_process)?;
        let image_path = process_image_path(raw_process)?;
        let image = open_live_image(&image_path)?;
        let executable_sha256 = hash_handle(&image)?;
        let mut exit_code = 0_u32;
        // SAFETY: raw_process remains a live query handle and exit_code is writable.
        if unsafe { GetExitCodeProcess(raw_process, &mut exit_code) } == 0
            || exit_code != STILL_ACTIVE as u32
        {
            return Err(LiveProcessError::InvalidProcess);
        }
        if listener_owner(endpoint)? != process_id {
            return Err(LiveProcessError::ListenerOwnerMismatch);
        }

        Ok(LiveServerIdentity {
            endpoint,
            process_id,
            creation_time_100ns,
            executable_sha256,
            _process: process,
            _image: image,
        })
    }

    pub(super) fn revalidate(
        endpoint: SocketAddr,
        identity: &LiveServerIdentity,
    ) -> Result<(), LiveProcessError> {
        revalidate_process(identity)?;
        if listener_owner(endpoint)? != identity.process_id {
            return Err(LiveProcessError::ListenerOwnerMismatch);
        }
        Ok(())
    }

    pub(super) fn verify_connected_stream(
        client_address: SocketAddr,
        server_address: SocketAddr,
        identity: &LiveServerIdentity,
    ) -> Result<(), LiveProcessError> {
        revalidate_process(identity)?;
        if established_owner(server_address, client_address)? != identity.process_id {
            return Err(LiveProcessError::ConnectionOwnerMismatch);
        }
        revalidate_process(identity)
    }

    fn revalidate_process(identity: &LiveServerIdentity) -> Result<(), LiveProcessError> {
        use std::os::windows::io::AsRawHandle;

        let process = identity._process.as_raw_handle();
        let mut exit_code = 0_u32;
        // SAFETY: the identity owns this query handle for its entire lifetime.
        if unsafe { GetExitCodeProcess(process, &mut exit_code) } == 0
            || exit_code != STILL_ACTIVE as u32
        {
            return Err(LiveProcessError::InvalidProcess);
        }
        if process_creation_time(process)? != identity.creation_time_100ns {
            return Err(LiveProcessError::ProcessCreationChanged);
        }
        Ok(())
    }

    fn process_creation_time(
        process: windows_sys::Win32::Foundation::HANDLE,
    ) -> Result<u64, LiveProcessError> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: process is a live query handle and all FILETIME out-pointers are valid.
        if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) }
            == 0
        {
            return Err(LiveProcessError::InvalidProcess);
        }
        let value = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        if value == 0 {
            return Err(LiveProcessError::InvalidProcess);
        }
        Ok(value)
    }

    fn process_image_path(
        process: windows_sys::Win32::Foundation::HANDLE,
    ) -> Result<PathBuf, LiveProcessError> {
        let mut buffer = vec![0_u16; 32_768];
        let mut length =
            u32::try_from(buffer.len()).map_err(|_| LiveProcessError::ImageUnavailable)?;
        // SAFETY: process is a live query handle and buffer has the declared UTF-16 capacity.
        if unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) } == 0
        {
            return Err(LiveProcessError::ImageUnavailable);
        }
        let length = usize::try_from(length).map_err(|_| LiveProcessError::ImageUnavailable)?;
        buffer.truncate(length);
        if buffer.is_empty() {
            return Err(LiveProcessError::ImageUnavailable);
        }
        Ok(PathBuf::from(OsString::from_wide(&buffer)))
    }

    fn open_live_image(path: &PathBuf) -> Result<File, LiveProcessError> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|_| LiveProcessError::ImageUnavailable)?;
        let metadata = file
            .metadata()
            .map_err(|_| LiveProcessError::ImageUnavailable)?;
        validate_image_metadata(metadata.is_file(), metadata.file_attributes())?;
        Ok(file)
    }

    fn validate_image_metadata(is_file: bool, attributes: u32) -> Result<(), LiveProcessError> {
        if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(LiveProcessError::ReparseImage);
        }
        if !is_file {
            return Err(LiveProcessError::ImageUnavailable);
        }
        Ok(())
    }

    fn hash_handle(file: &File) -> Result<[u8; 32], LiveProcessError> {
        let mut file = file
            .try_clone()
            .map_err(|_| LiveProcessError::ImageUnavailable)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|_| LiveProcessError::ImageUnavailable)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        buffer.fill(0);
        Ok(hasher.finalize().into())
    }

    fn listener_owner(endpoint: SocketAddr) -> Result<u32, LiveProcessError> {
        match endpoint.ip() {
            IpAddr::V4(address) => {
                let address = u32::from_ne_bytes(address.octets());
                find_listener_owner::<MIB_TCPROW_OWNER_PID>(
                    u32::from(AF_INET),
                    TCP_TABLE_OWNER_PID_LISTENER,
                    LiveProcessError::ListenerUnavailable,
                    |row| {
                        (row.dwLocalAddr == address || row.dwLocalAddr == 0)
                            && decode_port(row.dwLocalPort) == endpoint.port()
                    },
                    |row| row.dwOwningPid,
                )
            }
            IpAddr::V6(address) => {
                let octets = address.octets();
                find_listener_owner::<MIB_TCP6ROW_OWNER_PID>(
                    u32::from(AF_INET6),
                    TCP_TABLE_OWNER_PID_LISTENER,
                    LiveProcessError::ListenerUnavailable,
                    |row| {
                        (row.ucLocalAddr == octets || row.ucLocalAddr == [0; 16])
                            && decode_port(row.dwLocalPort) == endpoint.port()
                    },
                    |row| row.dwOwningPid,
                )
            }
        }
    }

    fn established_owner(
        server_address: SocketAddr,
        client_address: SocketAddr,
    ) -> Result<u32, LiveProcessError> {
        match (server_address, client_address) {
            (SocketAddr::V4(server), SocketAddr::V4(client)) => {
                let server_ip = u32::from_ne_bytes(server.ip().octets());
                let client_ip = u32::from_ne_bytes(client.ip().octets());
                find_listener_owner::<MIB_TCPROW_OWNER_PID>(
                    u32::from(AF_INET),
                    TCP_TABLE_OWNER_PID_CONNECTIONS,
                    LiveProcessError::ConnectionUnavailable,
                    |row| {
                        row.dwState == MIB_TCP_STATE_ESTAB as u32
                            && row.dwLocalAddr == server_ip
                            && decode_port(row.dwLocalPort) == server.port()
                            && row.dwRemoteAddr == client_ip
                            && decode_port(row.dwRemotePort) == client.port()
                    },
                    |row| row.dwOwningPid,
                )
            }
            (SocketAddr::V6(server), SocketAddr::V6(client)) => {
                let server_ip = server.ip().octets();
                let client_ip = client.ip().octets();
                find_listener_owner::<MIB_TCP6ROW_OWNER_PID>(
                    u32::from(AF_INET6),
                    TCP_TABLE_OWNER_PID_CONNECTIONS,
                    LiveProcessError::ConnectionUnavailable,
                    |row| {
                        row.dwState == MIB_TCP_STATE_ESTAB as u32
                            && row.ucLocalAddr == server_ip
                            && row.dwLocalScopeId == server.scope_id()
                            && decode_port(row.dwLocalPort) == server.port()
                            && row.ucRemoteAddr == client_ip
                            && row.dwRemoteScopeId == client.scope_id()
                            && decode_port(row.dwRemotePort) == client.port()
                    },
                    |row| row.dwOwningPid,
                )
            }
            _ => Err(LiveProcessError::ConnectionUnavailable),
        }
    }

    fn decode_port(value: u32) -> u16 {
        u16::from_be((value & u32::from(u16::MAX)) as u16)
    }

    fn find_listener_owner<Row: Copy>(
        address_family: u32,
        table_class: i32,
        unavailable: LiveProcessError,
        matches: impl Fn(&Row) -> bool,
        owner: impl Fn(&Row) -> u32,
    ) -> Result<u32, LiveProcessError> {
        let mut required = 0_u32;
        // SAFETY: a null table with zero size is the documented size query.
        let first = unsafe {
            GetExtendedTcpTable(
                ptr::null_mut(),
                &mut required,
                0,
                address_family,
                table_class,
                0,
            )
        };
        if first != ERROR_INSUFFICIENT_BUFFER || required < mem::size_of::<u32>() as u32 {
            return Err(unavailable);
        }
        let word = mem::size_of::<usize>();
        let words = usize::try_from(required)
            .map_err(|_| unavailable)?
            .div_ceil(word);
        let mut table = vec![0_usize; words];
        // SAFETY: table has at least required writable bytes and remains live during parsing.
        let loaded = unsafe {
            GetExtendedTcpTable(
                table.as_mut_ptr().cast(),
                &mut required,
                0,
                address_family,
                table_class,
                0,
            )
        };
        if loaded != NO_ERROR {
            return Err(unavailable);
        }
        let bytes = table.as_ptr().cast::<u8>();
        // SAFETY: a successful table begins with a u32 entry count.
        let count = unsafe { ptr::read_unaligned(bytes.cast::<u32>()) };
        let count = usize::try_from(count).map_err(|_| unavailable)?;
        let table_bytes = usize::try_from(required).map_err(|_| unavailable)?;
        let rows_bytes = count
            .checked_mul(mem::size_of::<Row>())
            .and_then(|bytes| bytes.checked_add(mem::size_of::<u32>()))
            .ok_or(unavailable)?;
        if rows_bytes > table_bytes {
            return Err(unavailable);
        }
        let rows = unsafe { bytes.add(mem::size_of::<u32>()) };
        for index in 0..count {
            // SAFETY: GetExtendedTcpTable returned count consecutive rows after the header.
            let row =
                unsafe { ptr::read_unaligned(rows.add(index * mem::size_of::<Row>()).cast()) };
            if matches(&row) {
                return Ok(owner(&row));
            }
        }
        Err(unavailable)
    }

    #[cfg(test)]
    mod tests {
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        use super::validate_image_metadata;
        use crate::LiveProcessError;

        #[test]
        fn reparse_image_attribute_is_never_accepted() {
            assert_eq!(
                validate_image_metadata(true, FILE_ATTRIBUTE_REPARSE_POINT).unwrap_err(),
                LiveProcessError::ReparseImage
            );
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::net::SocketAddr;

    use super::{LiveProcessError, LiveServerIdentity};

    pub(super) fn inspect(
        _endpoint: SocketAddr,
        _process_id: u32,
    ) -> Result<LiveServerIdentity, LiveProcessError> {
        Err(LiveProcessError::Unsupported)
    }

    pub(super) fn revalidate(
        _endpoint: SocketAddr,
        _identity: &LiveServerIdentity,
    ) -> Result<(), LiveProcessError> {
        Err(LiveProcessError::Unsupported)
    }

    pub(super) fn verify_connected_stream(
        _client_address: SocketAddr,
        _server_address: SocketAddr,
        _identity: &LiveServerIdentity,
    ) -> Result<(), LiveProcessError> {
        Err(LiveProcessError::Unsupported)
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LiveProcessError {
    #[error("remote or non-literal REST endpoints cannot be verified")]
    RemoteEndpoint,
    #[error("the REST endpoint changed after process binding")]
    EndpointMismatch,
    #[error("the REST listener is unavailable")]
    ListenerUnavailable,
    #[error("the REST listener belongs to a different process")]
    ListenerOwnerMismatch,
    #[error("the established REST connection is unavailable")]
    ConnectionUnavailable,
    #[error("the established REST connection belongs to a different process")]
    ConnectionOwnerMismatch,
    #[error("the signed server process is unavailable")]
    InvalidProcess,
    #[error("the signed server process creation identity changed")]
    ProcessCreationChanged,
    #[error("the live process image is unavailable")]
    ImageUnavailable,
    #[error("the live process image is a reparse point")]
    ReparseImage,
    #[error("live process verification is unsupported on this platform")]
    Unsupported,
}
