struct Pair {
    first: i32,
    second: i32
}

struct Values {
    first: i32,
    pair: Pair
}

fn main() -> i32 {
    let values = Values { first: 1, pair: Pair { first: 2, second: 3 } };
    let first = &values.first;
    let pair = &values.pair;
    let nested = &values.pair.first;
    *first = 4;
    let copied = *pair;
    *pair = Pair { first: 5, second: 6 };
    *nested = 7;
    let loaded = *pair;
    return *first + copied.second + loaded.first + loaded.second;
}
