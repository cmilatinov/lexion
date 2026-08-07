struct Large {
    first: i32,
    second: i32,
    third: i32,
    fourth: i32,
    fifth: i32
}

fn make_large() -> Large {
    return Large {
        first: 1,
        second: 2,
        third: 3,
        fourth: 4,
        fifth: 5
    };
}

fn main() -> i32 {
    let callback = make_large;
    let value = callback();
    return value.fifth;
}
