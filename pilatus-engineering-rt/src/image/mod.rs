mod logo;

pub(super) fn register_services(c: &mut minfac::ServiceCollection) {
    logo::register_services(c);
}
