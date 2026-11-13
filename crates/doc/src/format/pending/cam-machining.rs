//! Machining: a design may keep `cam`, its tool library (each tool may keep `details`: its
//! shank, lengths, corner or point, holder, cutting data per material and the shared library
//! entry it was copied from), setups (the body machined, the
//! material, the face on the table and the machine axes it gives (`table`, `axes`), the work
//! origin (`zero`: a point of the stock's box or of the model), the stock (margins, or
//! `fixed_stock` of a given size and placement) and clearance heights; an older setup's
//! `flipped` and `origin` are read as the same axes and origin) and operations (facing, pockets, contours and drilling, each naming its tool and the
//! faces or edges it cuts by a body and a point, and an offset from the bottom the model gives
//! it rather than a depth; a pocket or a contour may keep an `approach`: its entry, a contour's
//! leads and finishing passes). An older design has no `cam` and keeps no
//! machining. The last release warns about the newer format number and drops it.
