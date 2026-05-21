#let teacher_view = false

#set page(paper: "a4", flipped: true, 
  margin: (top: 0mm, bottom: 5mm, x: 10mm)
)

#set text(font: "BIZ UDGothic", size: 9pt, weight: 500)

#let seat(id, last_name, first_name, last_kana, first_kana) = {
  grid(columns: (5mm, auto), align: center, 
    align(top)[#id],
    table(columns: 35mm, 
    align: center+horizon, 
    stroke: (x,y) => {
      (left: 2pt)
      (right: 2pt)
      if y == 0 {
        (top: 2pt)
      }
      if y == 2 {
        (bottom: 2pt)
      }
      if y == 1 or y == 2 {
        (top: 1pt+gray)
      }
    }, 
      [#last_kana],
      text(size: 16pt)[#last_name],
      [#first_name (#first_kana)]
    )
  )
}

#let data = json("seats.json")

#let seats = {
  if teacher_view {
    data.seats
      .rev()
      .map(row => row.rev())
  } else {
    data.seats
  }
}

#align(center+horizon)[
  #align(left)[#text(size: 14pt)[#data.date～]]

  #{
    if not teacher_view {
      box(width: 80mm, height: 10mm, stroke: 2pt)[#text(size: 14pt)[教卓]]
    }
  }
  #move(dx: -2.5mm)[
    #grid(columns: data.layout.cols, align: center, inset: (x: 3mm, y: 2mm),
      ..seats.map(row =>
        row.map(id =>
          if id == none {
            ""
          } else {
            let s = data.students.at(str(id))
            seat(id, s.last_name, s.first_name, s.last_kana, s.first_kana)
          }
        )
      ).flatten()
    )
  ]
  #{
    if teacher_view {
      box(width: 80mm, height: 10mm, stroke: 2pt)[#text(size: 14pt)[教卓]]
    }
  }
]