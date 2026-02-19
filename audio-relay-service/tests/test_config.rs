use std::env;

use audio_relay_service::common::app_config::{
    AppConfig, AppConfigArgs, CONFIG_PATH_ENV, Environment,
};

use clap::Parser;

fn build_args(config_path: &str) -> AppConfigArgs {
    AppConfigArgs::parse_from(["test-bin", "--config", config_path])
}

#[test]
fn loads_valid_yaml_config() {
    let mut args = build_args("tests/resources/valid-test-config.yaml");

    let config = AppConfig::from_args(&mut args).unwrap();

    assert_eq!(config.environment, Environment::Development);
    assert_eq!(config.connection_limit, 100);
    assert_eq!(config.log_level, "info");
    assert_eq!(config.listen.to_string(), "[::1]:5555");
}

#[test]
fn cli_overrides_yaml_values() {
    unsafe { env::remove_var(CONFIG_PATH_ENV) };

    let mut args = AppConfigArgs::parse_from([
        "test-bin",
        "--config",
        "tests/resources/valid-test-config.yaml",
        "--connection-limit",
        "999",
    ]);

    let config = AppConfig::from_args(&mut args).unwrap();

    // CLI should override YAML (priority rule #1)
    assert_eq!(config.connection_limit, 999);
}

#[test]
fn env_var_overrides_cli_config_path() {
    // CLI path should be ignored
    unsafe { env::set_var(CONFIG_PATH_ENV, "tests/resources/valid-test-config.yaml") };
    let mut args = build_args("tests/resources/invalid-test-config-missing-field.yaml");

    let config = AppConfig::from_args(&mut args);
    if config.is_err() {
        unsafe { env::remove_var(CONFIG_PATH_ENV) };
        eprintln!("Loaded invalid test file... Check test failed or check valid-test-config.yaml");
        panic!("{:?}", config);
    }
    let config = config.unwrap();
    // It loaded the valid file from ENV instead
    assert_eq!(config.connection_limit, 100);

    unsafe { env::remove_var(CONFIG_PATH_ENV) };
}

#[test]
fn fails_on_invalid_yaml() {
    unsafe { env::remove_var(CONFIG_PATH_ENV) };

    let mut args = build_args("tests/resources/invalid-test-config-missing-field.yaml");

    let result = AppConfig::from_args(&mut args);

    assert!(result.is_err());
}

#[test]
fn fails_if_file_does_not_exist() {
    unsafe { env::remove_var(CONFIG_PATH_ENV) };

    let mut args = build_args("tests/resources/does-not-exist.yaml");

    let result = AppConfig::from_args(&mut args);
    println!("{:?}", result);
    assert!(result.is_err());
}
