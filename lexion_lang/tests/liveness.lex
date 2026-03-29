fn main(n: i32) -> i32 {
    let z = 10;
    let y = 0;
    let x = 1;
   
    while x < n {
        z = x * 2 + y;
        x = x + 1;
        y = x + z;
    }
    
    return z;
}
