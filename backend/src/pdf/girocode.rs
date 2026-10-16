// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! EPC-QR-Code ("GiroCode") for SEPA credit transfers.

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use rust_decimal::Decimal;

use crate::util::normalize_iban;

pub fn payload(name: &str, iban: &str, bic: &str, amount: Decimal, reference: &str) -> Option<String> {
    let iban = normalize_iban(iban);
    if iban.len() < 15 || amount <= Decimal::ZERO || amount > Decimal::from(999_999_999) {
        return None;
    }
    let name: String = name.chars().take(70).collect();
    let reference: String = reference.chars().take(140).collect();
    Some(format!(
        "BCD\n002\n1\nSCT\n{}\n{}\n{}\nEUR{:.2}\n\n\n{}",
        bic.trim().to_uppercase(),
        name,
        iban,
        amount.round_dp(2),
        reference
    ))
}

pub fn svg(data: &str) -> Option<Vec<u8>> {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M).ok()?;
    let image = code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .quiet_zone(false)
        .dark_color(svg::Color("#111111"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Some(image.into_bytes())
}
