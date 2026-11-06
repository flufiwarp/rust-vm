
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        println!(
            "\x1b[34m[scratch-vm] [i]\x1b[0m {}",
            format_args!($($arg)*)
        );
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        eprintln!(
            "\x1b[38;5;208m[scratch-vm] [!]\x1b[0m {}",
            format_args!($($arg)*)
        );
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        eprintln!(
            "\x1b[31m[scratch-vm] [!]\x1b[0m {}",
            format_args!($($arg)*)
        );
    };
}
