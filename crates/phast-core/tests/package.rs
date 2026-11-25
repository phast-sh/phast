use phast_core::package::PackageName;

// ---------------------------------------------------------------------------
// Which names are well formed
// ---------------------------------------------------------------------------

#[test]
fn a_vendor_package_pair_is_well_formed() {
    for name in [
        "psr/log",
        "symfony/polyfill-php80",
        "doctrine/dbal",
        "laminas/laminas-diactoros",
        "vendor_name/package.name",
        "0x/1",
    ] {
        assert!(PackageName::is_well_formed(name), "{name} was refused");
    }
}

#[test]
fn a_name_that_could_leave_its_directory_is_not_well_formed() {
    for name in [
        "../src",
        "vendor/..",
        "vendor/../../etc",
        "vendor/package/extra",
        "/etc/passwd",
        "vendor/",
        "/package",
    ] {
        assert!(!PackageName::is_well_formed(name), "{name} was accepted");
    }
}

#[test]
fn a_name_outside_composers_rules_is_not_well_formed() {
    for name in [
        "__root__",
        "php",
        "ext-json",
        "Vendor/Package",
        ".hidden/package",
        "vendor/package-",
        "vendor\\package",
        "vendor/pack age",
    ] {
        assert!(!PackageName::is_well_formed(name), "{name} was accepted");
    }
}
