//! L7-WP01: the link vocabulary every L7 surface reports in (lanes/L7_NETWORKING.md section 3.2,
//! 3.3). RED: the module body is absent, so its tests do not compile yet.

#[cfg(test)]
mod tests {
    include!("link_security_tests.rs");
}
