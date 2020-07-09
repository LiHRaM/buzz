use serde::Serialize;

#[derive(Serialize)]
pub struct Msg {
    pub text: String,
    pub tooltip: &'static str,
    pub class: &'static str,
    pub percentage: f64,
}
