#import sys: inputs

#let student_view = inputs.at("student_view", default: false)
#let teacher_view = inputs.at("teacher_view", default: false)

#set page(paper: "a4", flipped: true, 
  margin: (top: 5mm, bottom: 5mm, x: 5mm)
)

#set text(font: "UDEV Gothic", size: 9pt, weight: 500)

#let data = json(inputs.data)

#let tag-defs = data.at("tags", default: (:))

#let marks(student) = {
  let tags = student.at("tags", default: ())

  if tags.len() == 0 {
    return ""
  }

  tags.join(" ")
}

#let tag-table() = {
  if tag-defs.len() == 0 {
    []
  } else {
    table(columns: 2, align: left, stroke: none,
      ..tag-defs.keys().map(tag => (
        tag,
        tag-defs.at(tag, default: (:)).at("label", default: ""),
      )).flatten()
    )
  }
}

#let seat(id) = {
  let s = data.students.at(str(id))
  grid(columns: (4mm, auto, 4mm), align: center, inset: (x: .5mm),
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
    [
      #if s.last_kana != none {
        s.last_kana
      } else { " " }
    ],
    text(size: 14pt)[#s.last_name],
    [
      #s.first_name 
      #if s.first_kana != none {
        " (" + s.first_kana + ")"
      } else { " " }
    ]
    ),
    align(left+bottom)[#marks(s)]
  )
}

#let date = text(size: 11pt)[#data.date～]

#let seating-chart(teacher: false) = { 

  let seats = {
    if teacher {
      data.seats
        .rev()
        .map(row => row.rev())
    } else {
      data.seats
    }
  }

  align(center+horizon)[

    #{
      if not teacher {
        box(width: 81mm, stroke: 2pt, inset: (y: 1.5mm))[#align(center+horizon)[#text(size: 14pt)[教卓]]]
      }
    }
    #move(dx: 0mm)[
      #grid(columns: data.layout.cols, align: center, inset: (x: 1mm, y: 1.5mm),
        ..seats.map(row =>
          row.map(id =>
            if id == none {
              ""
            } else {
              let s = data.students.at(str(id))
              seat(id)
            }
          )
        ).flatten()
      )
    ]
    #{
      if teacher {
        box(width: 81mm, stroke: 2pt, inset: (y: 1.5mm))[#align(center+horizon)[#text(size: 14pt)[教卓]]]
      }
    }
  ]
}

#if student_view {
  date
  seating-chart()
  tag-table()
}

#pagebreak(weak:true)

#if teacher_view {
  date
  seating-chart(teacher: true)
  tag-table()
}
