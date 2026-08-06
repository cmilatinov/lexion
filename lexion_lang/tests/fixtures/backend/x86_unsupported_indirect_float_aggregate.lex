struct Mixed {
    first: i32,
    second: f32,
    third: i32,
    fourth: i32,
    fifth: i32
}

fn unsupported() -> Mixed {
    return Mixed { first: 1, second: 2.0, third: 3, fourth: 4, fifth: 5 };
}
