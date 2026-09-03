//! Facade coverage for opt-in image decoding.

#![cfg(feature = "image-decoding")]

#[test]
fn facade_exposes_opt_in_image_decoding() {
    let decode: fn(&[u8]) -> Result<arborui::RgbaImage, arborui::image_decoder::DecodeError> =
        arborui::image_decoder::decode;

    let error = decode(b"not an encoded image").expect_err("invalid image data must fail");
    assert_eq!(
        error.kind(),
        arborui::image_decoder::DecodeErrorKind::Unsupported
    );
}
