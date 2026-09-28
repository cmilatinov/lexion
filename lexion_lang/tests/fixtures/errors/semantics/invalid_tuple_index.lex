extern fn pair() -> (i32, bool);

fn main() {
    let value = pair()[0];
}
