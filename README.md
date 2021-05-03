# nextcloud-config-parser

Rust parser for nextcloud config files.

## Usage

```rust
use nextcloud_config_parser::{parse, Error};

fn main() -> Result<(), Error> {
    let config = parse("tests/configs/basic.php")?;
    dbg!(config);

    Ok(())
}

```