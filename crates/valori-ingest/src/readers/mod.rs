// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
pub mod docx;
pub mod html;
pub mod markdown;
pub mod pdf;

pub use docx::DocxReader;
pub use html::HtmlReader;
pub use markdown::MarkdownReader;
pub use pdf::PdfReader;
