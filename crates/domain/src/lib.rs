//! Pure domain logic: classification, filters, sorting, trade-up math.

pub mod classification;

pub use classification::{CATEGORIES, MAJORS, classify_category, classify_major};
