use zero_ui::{widget::Wgt, APP};

fn main() {
    let _scope = APP.minimal();
    let _ = Wgt! {
        zero_ui::layout:: = 0;
    };
}
