fn choose(flag: bool) -> i32 {
    let value = if flag { 1 } else { 2 };
    return value;
}

fn main() {
    let block_value: i32 = { 1 };
    if true {
        let scoped = block_value;
    };
    while false {
        let loop_value = choose(true);
    }
}
