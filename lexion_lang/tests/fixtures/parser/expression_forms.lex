fn choose(flag: bool) -> i32 {
    return if flag { 1 } else { 2 };
}

fn main() {
    let x = { choose(true) };
    if true { let y = x; } else { let y = 0; }
    let z = x;
}
