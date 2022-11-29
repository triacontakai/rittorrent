/// Safe wrapper around `libc::strerror_r`
pub fn strerror() -> String {
    // Safety: __errno_location will never fail
    let errno = unsafe { *libc::__errno_location() };

    let mut buf = vec![0i8; 128];

    // Safety: buf.len() ensures that there will be no OOB write
    // buf also outlives this block, which makes as_mut_ptr fine
    unsafe { libc::strerror_r(errno, buf.as_mut_ptr(), buf.len()) };

    // crate the string
    // this should never fail, since strerror places an ascii string into buf
    String::from_utf8(
        buf.into_iter()
            .take_while(|c| *c != 0)
            .map(|c| c as u8)
            .collect(),
    )
    .expect("strerror returned invalid utf-8")
}
