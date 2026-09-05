//! arXiv metadata取り込み（アーキテクチャ.txt 5.4 の "arXiv metadata fetch" 段）。
//! Phase 1のスコープは title/abstract/authors/categories/msc-class のみで、
//! full text PDFの解析は行わない。

pub mod model;
pub mod oai;
pub mod store;
