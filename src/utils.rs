//! Common utility functions for the application.

/// Calculate compression ratio as percentage reduction.
#[must_use]
pub fn compression_ratio(original: usize, compressed: usize) -> f64 {
    if original == 0 {
        return 0.0;
    }
    100.0 - (compressed as f64 / original as f64 * 100.0)
}

/// Format byte size as human-readable string.
#[must_use]
pub fn format_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_ratio() {
        assert_eq!(compression_ratio(100, 50), 50.0);
        assert_eq!(compression_ratio(1000, 100), 90.0);
        assert_eq!(compression_ratio(0, 0), 0.0);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
    }
}
