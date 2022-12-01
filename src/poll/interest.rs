use std::ops::{BitOr, BitOrAssign};

/// Thin wrapper around epoll bitflags
///
/// Currently only supports EPOLLIN and EPOLLOUT (with [Interest::READABLE] and [Interest::WRITABLE] respectively)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Interest(u32);

impl Interest {
    pub const READABLE: Interest = Interest(libc::EPOLLIN as u32);
    pub const WRITABLE: Interest = Interest(libc::EPOLLOUT as u32);

    /// Returns epoll event flags
    pub fn flags(&self) -> u32 {
        self.0
    }
}

impl BitOr for Interest {
    type Output = Interest;

    fn bitor(self, rhs: Self) -> Self {
        Interest(self.0 | rhs.0)
    }
}

impl BitOrAssign for Interest {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}