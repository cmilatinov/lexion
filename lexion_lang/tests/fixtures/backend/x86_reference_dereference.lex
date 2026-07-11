fn main() -> i32 {
    let value = 1;
    let reference = &value;
    *reference = 2;
    return *reference;
}
