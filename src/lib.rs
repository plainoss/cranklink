pub mod ble;
#[cfg(not(target_arch = "wasm32"))]
pub mod bootloader_dump;
pub mod device;
pub mod firmware;
mod firmware_io;
pub mod protocol;
#[cfg(not(target_arch = "wasm32"))]
pub mod recovery;

pub mod rx;

pub mod settings;

pub mod cli;
pub mod platform;
pub mod storage;
#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub mod motor_support;
pub mod unit_models;

#[cfg(test)]
mod tests {
    #[test]
    fn wasm_bindgen_crate_pin_matches_the_cli_builds() {
        // The web build fails with a schema mismatch unless the wasm-bindgen
        // CLI matches the locked crate exactly.
        let pin = include_str!("../Cargo.toml")
            .lines()
            .find_map(|line| line.strip_prefix("wasm-bindgen = \"=")?.strip_suffix('"'))
            .expect("Cargo.toml pins wasm-bindgen with an exact =version");
        assert!(
            include_str!("../web-instrument/scripts/build-cloudflare.sh")
                .lines()
                .any(|line| line == format!("WASM_BINDGEN_VERSION={pin}")),
            "build-cloudflare.sh must install wasm-bindgen {pin}"
        );
        assert!(
            include_str!("../.github/workflows/web.yml")
                .lines()
                .any(|line| line.trim() == format!("tool: wasm-bindgen@{pin}")),
            "web.yml must install wasm-bindgen {pin}"
        );
    }
}
