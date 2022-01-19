use miette::Result;
use nextcloud_config_parser::parse;

fn main() -> Result<()> {
    let config = parse("tests/configs/basic.php")?;
    dbg!(config);
    Ok(())
}
