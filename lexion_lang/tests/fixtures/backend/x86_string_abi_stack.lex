fn echo(value: &str) -> &str {
    return value;
}

fn take(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, value: &str) -> &str {
    return echo(value);
}

fn main() -> i32 {
    let value = take(1, 2, 3, 4, 5, 6, "hello");
    return 0;
}
