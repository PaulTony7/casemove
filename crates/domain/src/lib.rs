//! Pure domain logic: classification, filters, sorting, trade-up math.

pub mod classification;
pub mod tradeup;

pub use classification::{
    CATEGORIES, MAJORS, classify_category, classify_major, trade_unlock_ms,
};
pub use tradeup::{
    CandidateItem, Collections, Contract, ContractFilter, Outcome, is_trade_up_skin, outcomes,
    picker_matches, rarity_name, wear_for,
};
