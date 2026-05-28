extern fn pair() -> (i32, bool);

fn main() {
    let character: char = "abc"[0];
    let first: i32 = pair().0;
    let second: bool = pair().1;
    let integer: i32 = true as i32;
    let float: f32 = integer as f32;
    let same: i32 = -1 as i32;
}
