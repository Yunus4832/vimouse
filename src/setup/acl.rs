//! Linux POSIX ACL xattrs: grant one account without requiring libacl/setfacl.
use std::{io, os::fd::RawFd};

const UNDEFINED: u32 = u32::MAX;
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    tag: u16,
    perm: u16,
    id: u32,
}

fn grant(existing: &[u8], mode: u32, uid: u32) -> io::Result<Vec<u8>> {
    let mut entries = if existing.is_empty() {
        vec![
            Entry {
                tag: 1,
                perm: ((mode >> 6) & 7) as u16,
                id: UNDEFINED,
            },
            Entry {
                tag: 4,
                perm: ((mode >> 3) & 7) as u16,
                id: UNDEFINED,
            },
            Entry {
                tag: 32,
                perm: (mode & 7) as u16,
                id: UNDEFINED,
            },
        ]
    } else {
        if existing.len() < 4
            || (existing.len() - 4) % 8 != 0
            || existing[..4] != 2u32.to_le_bytes()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid POSIX ACL xattr",
            ));
        }
        existing[4..]
            .chunks_exact(8)
            .map(|e| Entry {
                tag: u16::from_le_bytes(e[..2].try_into().unwrap()),
                perm: u16::from_le_bytes(e[2..4].try_into().unwrap()),
                id: u32::from_le_bytes(e[4..].try_into().unwrap()),
            })
            .collect::<Vec<_>>()
    };
    if entries
        .iter()
        .any(|e| !matches!(e.tag, 1 | 2 | 4 | 8 | 16 | 32) || e.perm > 7)
        || [1, 4, 32]
            .iter()
            .any(|tag| entries.iter().filter(|e| e.tag == *tag).count() != 1)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported POSIX ACL entries",
        ));
    }
    let mask = entries.iter().find(|e| e.tag == 16).map_or_else(
        || entries.iter().find(|e| e.tag == 4).unwrap().perm,
        |e| e.perm,
    );
    // Expanding the mask must not accidentally grant new rights to other users/groups.
    for e in entries.iter_mut().filter(|e| matches!(e.tag, 2 | 4 | 8)) {
        e.perm &= mask;
    }
    if let Some(e) = entries.iter_mut().find(|e| e.tag == 2 && e.id == uid) {
        e.perm |= 6;
    } else {
        entries.push(Entry {
            tag: 2,
            perm: 6,
            id: uid,
        });
    }
    if let Some(e) = entries.iter_mut().find(|e| e.tag == 16) {
        e.perm |= 6;
    } else {
        entries.push(Entry {
            tag: 16,
            perm: mask | 6,
            id: UNDEFINED,
        });
    }
    entries.sort_by_key(|e| (e.tag, e.id));
    let mut data = 2u32.to_le_bytes().to_vec();
    for e in entries {
        data.extend(e.tag.to_le_bytes());
        data.extend(e.perm.to_le_bytes());
        data.extend(e.id.to_le_bytes());
    }
    Ok(data)
}

pub fn grant_fd(fd: RawFd, uid: u32) -> io::Result<()> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fd is borrowed from a live opened device; stat points to writable storage.
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstat succeeded and initialized stat.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFCHR {
        return Err(io::Error::other("refusing ACL changes on a non-device"));
    }
    if stat.st_uid == uid {
        // Named ACL entries never override owner permissions.
        // SAFETY: the borrowed fd is valid; preserve all existing mode bits.
        if unsafe { libc::fchmod(fd, stat.st_mode | 0o600) } != 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(());
    }
    let name = c"system.posix_acl_access";
    let mut bytes = vec![0u8; 65536];
    // SAFETY: fd and C string are valid, bytes is a writable buffer of the given length.
    let length =
        unsafe { libc::fgetxattr(fd, name.as_ptr(), bytes.as_mut_ptr().cast(), bytes.len()) };
    if length < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENODATA) {
            return Err(error);
        }
        bytes.clear();
    } else {
        bytes.truncate(length as usize);
    }
    let updated = grant(&bytes, stat.st_mode, uid)?;
    // SAFETY: updated lives through this call and contains a validated ACL encoding.
    if unsafe { libc::fsetxattr(fd, name.as_ptr(), updated.as_ptr().cast(), updated.len(), 0) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entries(bytes: &[u8]) -> Vec<(u16, u16, u32)> {
        bytes[4..]
            .chunks_exact(8)
            .map(|e| {
                (
                    u16::from_le_bytes(e[..2].try_into().unwrap()),
                    u16::from_le_bytes(e[2..4].try_into().unwrap()),
                    u32::from_le_bytes(e[4..].try_into().unwrap()),
                )
            })
            .collect()
    }
    #[test]
    fn existing_effective_rights_are_preserved_and_grant_is_idempotent() {
        let initial = grant(&[], 0o640, 1000).unwrap();
        let mut acl = entries(&initial);
        assert!(acl.contains(&(2, 6, 1000)));
        assert!(acl.contains(&(4, 4, UNDEFINED)));
        // Simulate an administrator masking a broad named-user entry to read-only.
        acl.iter_mut().find(|e| e.0 == 16).unwrap().1 = 4;
        let mut old = 2u32.to_le_bytes().to_vec();
        for (tag, perm, id) in acl {
            old.extend(tag.to_le_bytes());
            old.extend(perm.to_le_bytes());
            old.extend(id.to_le_bytes());
        }
        let next = grant(&old, 0o640, 1001).unwrap();
        let acl = entries(&next);
        assert!(acl.contains(&(2, 4, 1000)));
        assert!(acl.contains(&(2, 6, 1001)));
        assert_eq!(grant(&next, 0o640, 1001).unwrap(), next);
        assert!(grant(&[0, 1, 2], 0o640, 1001).is_err());
    }
    #[test]
    fn linux_accepts_the_encoding_on_an_owned_temporary_file() {
        use std::{
            fs::{self, OpenOptions},
            os::{
                fd::AsRawFd,
                unix::fs::{DirBuilderExt, OpenOptionsExt},
            },
        };
        let directory =
            std::env::temp_dir().join(format!("vimouse-acl-test-{}", std::process::id()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let path = directory.join("file");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        let fd = file.as_raw_fd();
        let data = grant(&[], 0o600, unsafe { libc::geteuid() } + 1).unwrap();
        // Test only an owned temporary regular file, never a real input device.
        let result = unsafe {
            libc::fsetxattr(
                fd,
                c"system.posix_acl_access".as_ptr(),
                data.as_ptr().cast(),
                data.len(),
                0,
            )
        };
        let error = std::io::Error::last_os_error();
        let mut readback = vec![0u8; data.len()];
        let count = if result == 0 {
            unsafe {
                libc::fgetxattr(
                    fd,
                    c"system.posix_acl_access".as_ptr(),
                    readback.as_mut_ptr().cast(),
                    readback.len(),
                )
            }
        } else {
            -1
        };
        drop(file);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
        if result != 0 && error.raw_os_error() == Some(libc::EOPNOTSUPP) {
            return;
        }
        assert_eq!(result, 0, "{error}");
        assert_eq!(count as usize, data.len());
        assert_eq!(readback, data);
    }
}
