use zero_ui::{widget::Wgt, APP};

fn main() {
    let _scope = APP.minimal();
    let _ = Wgt! {
        unknown = {
            value: 0,
        };
    };
}
