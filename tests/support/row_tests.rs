/// Declares one `#[test]` per fixture row, row range or share of the rows, so long
/// fixture loops run in parallel and a failure names its rows:
/// `row_tests!(run; row_00(0), row_01(1));`
/// expands to `#[test] fn row_00() { run(0) }` and so on.
macro_rules! row_tests {
    ($run:ident; $($name:ident($($arg:expr),*)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                $run($($arg),*);
            }
        )+
    };
}
