pub mod docx;
pub mod html;
pub mod markdown;
pub mod pdf;
pub mod text;

pub use docx::DocxExtractor;
pub use html::HtmlExtractor;
pub use markdown::MarkdownExtractor;
pub use pdf::PdfExtractor;
pub use text::TextExtractor;
