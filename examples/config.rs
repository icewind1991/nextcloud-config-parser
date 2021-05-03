use nextcloud_config_parser::parse;

fn main() {
    let config = match parse("tests/configs/basic.php") {
        Ok(config) => config,
        Err(err) => {
            eprintln!("{}", err);
            return;
        }
    };
    dbg!(config);
}
