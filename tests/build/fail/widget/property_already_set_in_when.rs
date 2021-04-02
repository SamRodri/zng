use zero_ui::core::widget;

#[widget($crate::test_widget)]
pub mod test_widget {
    use zero_ui::properties::{margin, states::is_pressed};

    properties! {
        margin = 0;

        when self.is_pressed {
            margin = 1;
            margin = 2;
        }
    }
}

fn main() {}
