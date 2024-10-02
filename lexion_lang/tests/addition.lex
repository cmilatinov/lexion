fn main(in: u32) -> u32 {
    let a = 0;
    let b = &a;
    (*b) = 12;
}
