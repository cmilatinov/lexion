fn accept(first: i32, second: i32, third: i32, value: &str) -> i32 {
    return third;
}

fn main() -> i32 {
    let first = 1;
    let second = 2;
    let third = 3;
    return accept(first, second, third, "hello");
}
