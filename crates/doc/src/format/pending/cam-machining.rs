//! Machining: a design may keep `cam`, its tool library, setups (the body machined, the
//! material, the work origin, whether the part is turned over, the stock margins and clearance
//! heights) and operations (facing, pockets, contours and drilling, each naming its tool and the
//! faces or edges it cuts by a body and a point, and an offset from the bottom the model gives
//! it rather than a depth). An older design has no `cam` and keeps no
//! machining. The last release warns about the newer format number and drops it.
