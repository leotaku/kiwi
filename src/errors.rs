#[derive(Debug)]
pub struct EmptyError;

impl std::error::Error for EmptyError {}

impl std::fmt::Display for EmptyError {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}
