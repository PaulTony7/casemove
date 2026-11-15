//! CSV export of inventory/storage rows, matching the legacy
//! `downloadReport` layout: one header line, then JSON-quoted fields for
//! every row with `null` stripped. Storage rows carry their unit's name;
//! inventory rows fall back to `#Inventory`.

use crate::inventory::GroupedItem;

pub const HEADER: &str = "Item Name,Item Custom Name,Price,Price Combined,Item Moveable,Storage Name,Tradehold,Category,Combined QTY,Item Wear Name,Item Paint Wear,Item Has Stickers/Patches,Stickers\n";

/// One grouped row with the storage unit it lives in (`None` = inventory).
pub struct Row<'a> {
    pub grouped: &'a GroupedItem,
    pub storage_name: Option<&'a str>,
    /// Whether the item is still trade-locked (legacy `trade_unlock`).
    pub trade_locked: bool,
}

pub fn export(rows: &[Row<'_>], currency: &pricing::Currency) -> String {
    let mut out = String::with_capacity(HEADER.len() + rows.len() * 200);
    out.push_str(HEADER);
    for entry in rows {
        let item = &entry.grouped.representative;
        let price = item
            .price
            .map(|price| pricing::format_currency(currency, price, 2));
        let combined = item.price.map(|price| {
            pricing::format_currency(currency, price * entry.grouped.combined_qty as f64, 2)
        });
        let stickers: Vec<&str> = item
            .stickers
            .iter()
            .map(|sticker| sticker.name.as_str())
            .collect();
        let fields = [
            field(&item.name),
            field(item.custom_name.as_deref().unwrap_or("")),
            price.map(|price| field(&price)).unwrap_or_default(),
            combined.map(|price| field(&price)).unwrap_or_default(),
            field(if item.moveable { "true" } else { "false" }),
            field(entry.storage_name.unwrap_or("#Inventory")),
            field(if entry.trade_locked { "true" } else { "false" }),
            field(item.category.as_deref().unwrap_or("")),
            field(&entry.grouped.combined_qty.to_string()),
            field(item.wear_name.as_deref().unwrap_or("")),
            match item.wear {
                Some(wear) => field(&format!("{wear:.6}")),
                None => String::new(),
            },
            field(if item.stickers.is_empty() { "false" } else { "true" }),
            field(&stickers.join(";")),
        ];
        out.push('[');
        out.push_str(&fields.join(","));
        out.push_str("]\n");
    }
    out
}

/// `JSON.stringify` of a string, matching the legacy per-field quoting.
fn field(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_commands::{InventoryItem, InventorySticker};

    fn grouped() -> GroupedItem {
        GroupedItem {
            representative: InventoryItem {
                id: "1".into(),
                definition_index: 7,
                name: "AK-47 | Redline".into(),
                custom_name: Some("Renamed \"tag\"".into()),
                image_url: None,
                image_path: None,
                quantity: 1,
                rarity: 3,
                position: 1,
                wear: Some(0.1234567),
                wear_name: Some("Field-Tested".into()),
                paint_seed: None,
                stat_trak: false,
                is_storage_unit: false,
                storage_item_count: None,
                category: Some("Skins & Knives".into()),
                major: None,
                trade_unlock: None,
                moveable: true,
                equipped_ct: false,
                equipped_t: false,
                price: Some(10.5),
                collection: None,
                trade_up_eligible: true,
                stickers: vec![InventorySticker {
                    slot: 0,
                    name: "Sticker | Foo".into(),
                    kind: "Sticker".into(),
                    image_path: None,
                    image_url: None,
                    wear: None,
                    scale: None,
                    rotation: None,
                    offset_x: None,
                    offset_y: None,
                }],
                music_kit: None,
                graffiti: None,
            },
            combined_ids: vec!["1".into(), "2".into()],
            combined_qty: 2,
        }
    }

    #[test]
    fn writes_the_legacy_header_and_quoted_fields() {
        let row = grouped();
        let csv = export(
            &[Row {
                grouped: &row,
                storage_name: None,
                trade_locked: false,
            }],
            &pricing::Currency::usd(),
        );
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some(HEADER.trim_end()));
        let line = lines.next().expect("one data row");
        assert_eq!(line.matches("\",\"").count(), 12, "{line}");
        assert!(line.starts_with(r#"["AK-47 | Redline","Renamed \"tag\"","#), "{line}");
        assert!(line.contains(r#""$10.50","$21.00""#), "{line}");
        assert!(line.contains(r#""true","#), "{line}");
        assert!(line.ends_with(r#"Skins & Knives","2","Field-Tested","0.123457","true","Sticker | Foo"]"#), "{line}");
        assert!(line.contains("Inventory"), "{line}");
    }

    #[test]
    fn missing_prices_and_wear_leave_empty_fields() {
        let mut row = grouped();
        row.representative.price = None;
        row.representative.wear = None;
        row.representative.stickers.clear();
        let csv = export(
            &[Row {
                grouped: &row,
                storage_name: Some("Unit A"),
                trade_locked: true,
            }],
            &pricing::Currency::usd(),
        );
        let line = csv.lines().nth(1).unwrap();
        assert!(
            line.starts_with(r#"["AK-47 | Redline","Renamed \"tag\"",,,"true","Unit A","#),
            "{line}"
        );
        // Sticker list is the final field; empty wear leaves a blank slot.
        assert!(line.ends_with(r#",,"false",""]"#), "{line}");
    }
}
