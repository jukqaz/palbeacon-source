fn main() -> Result<(), Box<dyn std::error::Error>> {
    pal_test_window::run_from_args(std::env::args().skip(1))
}
