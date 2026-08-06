struct Large {
    first: i32,
    second: i32,
    third: i32,
    fourth: i32,
    fifth: i32
}

struct Triple {
    first: i32,
    second: i32,
    third: i32
}

fn pack(a: i32, b: i32, c: i32, d: i32, e: i32, tail: i32, triple: Triple) -> Large {
    return Large {
        first: a + tail,
        second: b + triple.first,
        third: c + triple.second,
        fourth: d + triple.third,
        fifth: e
    };
}

fn main() -> i32 {
    let value = pack(1, 2, 3, 4, 5, 6, Triple { first: 7, second: 8, third: 9 });
    return value.first + value.second + value.third + value.fourth + value.fifth;
}
