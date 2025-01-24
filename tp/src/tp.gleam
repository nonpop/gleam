import gleam

pub type X {
  Ok
}

fn func(x) {
  case gleam.Ok {
    _ if [] == [gleam.Ok] -> True
    _ -> False
  }
}
