fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let total: i32 = 0;
    let i: i32 = 0;
    while i < 3 {
        total = total + add_one(i);
        i = i + 1;
    }
    let selected: i32 = 0;
    if total > 3 {
        selected = total;
    } else {
        selected = 0;
    }
    return selected;
}
