struct Triple {
    first: i32,
    second: i32,
    third: i32
}

fn main() -> i32 {
    let value = Triple { first: 1, second: 2, third: 3 };
    let reference = &value;
    let copy = *reference;
    *reference = Triple { first: 4, second: 5, third: 6 };
    let loaded = *reference;
    return copy.first + loaded.second + loaded.third;
}
