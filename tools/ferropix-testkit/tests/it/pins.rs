// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

use ferropix_testkit::containers::IMAGES;
use ferropix_testkit::pins::matrix_pin;

#[test]
fn every_image_is_pinned_in_the_matrix_by_its_digest() {
    for image in IMAGES {
        let pin = matrix_pin(image.item).unwrap();
        assert_eq!(
            image.reference(),
            pin,
            "{} drifted from docs/VERSIONS.md",
            image.item
        );
    }
}

#[test]
fn the_harness_starts_no_image_outside_the_pinned_set() {
    let source = include_str!("../../src/containers.rs");
    assert_eq!(
        1,
        source.matches("GenericImage::new(").count(),
        "every image is built by PinnedImage::image alone"
    );
}

#[test]
fn the_store_image_is_postgresql_18() {
    let postgres = IMAGES
        .iter()
        .find(|image| image.repository == "postgres")
        .unwrap();
    assert!(
        postgres.tag.starts_with("18."),
        "the store is PostgreSQL 18, got {}",
        postgres.tag
    );
}
