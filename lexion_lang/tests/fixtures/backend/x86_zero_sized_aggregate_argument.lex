struct Empty {}

fn consume(empty: Empty, value: i32) -> i32 {
    return value;
}

fn main() -> i32 {
    return consume(Empty {}, 7);
}
