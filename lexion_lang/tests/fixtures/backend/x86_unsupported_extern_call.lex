extern fn imported(value: i32) -> i32;
fn main() -> i32 {
    return imported(1);
}
