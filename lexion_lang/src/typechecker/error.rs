use lexion_lib::tokenizer::SourceLocation;

#[derive()]
pub struct TypeError {
    pub loc: SourceLocation,
    pub message: String,
}
