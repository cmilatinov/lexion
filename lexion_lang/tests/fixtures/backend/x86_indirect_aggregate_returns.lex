struct Large {
    first: i32,
    second: i32,
    third: i32,
    fourth: i32,
    fifth: i32
}

fn make_large(base: i32, extra: i32) -> Large {
    return Large {
        first: base,
        second: base + 1,
        third: base + 2,
        fourth: base + 3,
        fifth: extra
    };
}

fn make_tuple(base: i32, extra: i32) -> (i32, i32, i32, i32, i32) {
    return (base, base + 1, base + 2, base + 3, extra);
}

fn main() -> i32 {
    let large = make_large(10, 20);
    let tuple = make_tuple(30, 40);
    return large.first + large.fifth + tuple.0 + tuple.4;
}
