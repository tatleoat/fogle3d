// SPDX-License-Identifier: MIT
//! The records of the user-interface toolkit's bottom layer, with the original's 32-bit
//! layout (finding 0021). Every record is `#[repr(C)]`, its size and each field's offset are
//! asserted at compile time, and its fields tile it without padding, so it can be read and
//! written in place in the memory image ([`crate::mem`]).
//!
//! Field names are ours; the structure names are the binary's (from mangled parameter
//! types). A field called `u_XXX` covers bytes at offset 0xXXX that these files do not touch
//! or whose meaning was not read; the finding says for each named field whether it is read
//! from code, from a name, or a guess.

use crate::mem::{Addr, P};
use core::mem::{offset_of, size_of};

/// Defines a record: `OFFSET name: type, "what it is";` per field.
macro_rules! record {
    (
        $(#[$meta:meta])*
        pub struct $name:ident = $orig:literal, size $size:literal {
            $( $off:literal $field:ident : $ty:ty , $doc:literal ; )*
        }
    ) => {
        $(#[$meta])*
        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name {
            $( #[doc = $doc] pub $field : $ty , )*
        }

        // every field is Pod and (asserted below) the fields tile the structure
        unsafe impl $crate::mem::Pod for $name {}

        impl $name {
            /// Size in the original, in bytes.
            pub const SIZE: u32 = $size;
            /// The structure's name in the binary.
            pub const ORIGINAL: &'static str = $orig;
            /// Offset, size, name and description of every field.
            pub const FIELDS: &'static [(u32, u32, &'static str, &'static str)] =
                &[ $( ($off, size_of::<$ty>() as u32, stringify!($field), $doc) ),* ];
        }

        const _: () = {
            assert!(size_of::<$name>() == $size);
            $( assert!(offset_of!($name, $field) == $off); )*
            assert!(0 $( + size_of::<$ty>() )* == $size);
        };
    };
}

record! {
    /// The program's event record, 0x18 bytes [soft 0x08393bc4 newEvent]. What the four
    /// 16-bit fields and the pointer hold depends on the type; see [`ev`].
    pub struct Event = "SI_Event", size 0x18 {
        0x00 kind: i32, "the event's type (`SI_EventType`), 0 for none";
        0x04 first: i16, "group identifier; or the device (dialog hit, learn mode); or the queue value (view hit)";
        0x06 second: i16, "control identifier, -1 for a group alone; or the queue value; or the device (view hit, puck)";
        0x08 x: i16, "pointer x in screen coordinates";
        0x0a y: i16, "pointer y in screen coordinates (IRIS GL: upwards from the lower edge)";
        0x0c data: Addr, "the group record, the dialog record or the action string the event is about";
        0x10 extra: i16, "written by the menu code (the fifth output of SI_popupFindLastSelect), zeroed by the swift-key path";
        0x12 u_012: [u8; 2], "padding";
        0x14 device: i32, "the IRIS GL device read from the queue for this event";
    }
}

record! {
    /// A scroll-bar or text-list control, `SI_Scroll`, as far as `scroll.c` reads it. The
    /// record is larger than this (`SI_scrollRead` writes at +0x20 and +0x24); the bytes
    /// below +0x16 that no ported function reads are `u_XXX`. Layout read from the code that
    /// draws it; the names are ours.
    pub struct Scroll = "SI_Scroll", size 0x18 {
        0x00 kind: i32, "the kind of control, the index of `SI_scrollSize`'s jump table (0 to 13)";
        0x04 u_004: [u8; 2], "not read";
        0x06 x1: i16, "left edge, screen coordinates";
        0x08 x2: i16, "right edge; for the text list the scroll bar sits right of it";
        0x0a y1: i16, "bottom edge";
        0x0c y2: i16, "top edge";
        0x0e u_00e: [u8; 2], "not read";
        0x10 data: Addr, "the control's data: a [`ScrollData`] for the text list and its bar; for kinds 3 and 4 a pointer to the label string pointer";
        0x14 state: i16, "1 when the control is greyed out (drawn in the ghost colour)";
        0x16 u_016: [u8; 2], "not read";
    }
}

record! {
    /// The data of a text list and its scroll bar, pointed to by [`Scroll::data`]
    /// (`scroll.c`'s draw functions). Names ours.
    pub struct ScrollData = "(scroll.c list data)", size 0x14 {
        0x00 items: Addr, "the array of [`ScrollItem`], 0x14 bytes each";
        0x04 flag: i16, "passed on to the bar's draw as its third argument (0: the arrows only, otherwise bar and thumb)";
        0x06 first: i16, "the first visible row (also the thumb's offset in rows)";
        0x08 count: i16, "the number of items";
        0x0a mode: i16, "the bar's colour mode: 0, 1 or 2 pick which parts are drawn dark, any other value draws them in colour 0";
        0x0c visible: i16, "the number of rows that fit the list";
        0x0e u_00e: [u8; 2], "not read";
        0x10 scale: f32, "the thumb's pixels per row";
    }
}

record! {
    /// One row of a text list (`scroll.c`'s `scrollTextViewDrawGL`). Names ours.
    pub struct ScrollItem = "(scroll.c list item)", size 0x14 {
        0x00 a: i16, "non-zero: the row is selectable (drawn in the window text colour, and with the select write mask in the front buffer)";
        0x02 b: i16, "non-zero: the row is greyed or selected, by `a`";
        0x04 text: Addr, "the row's text, null for none";
        0x08 u_008: [u8; 4], "not read";
        0x0c colour: Addr, "pointer to the 16-bit colour index the row is drawn in (front buffer)";
        0x10 callback: Addr, "a function called after the row is drawn with the items, the row, the text's x and y; null for none";
    }
}

/// The event types these two files produce (the values of `SI_EventType` seen; the names are
/// ours, there is no symbol for them).
pub mod ev {
    /// Nothing to do.
    pub const NONE: i32 = 0;
    /// Leave the program (the Exit button, confirmed).
    pub const QUIT: i32 = 4;
    /// A mouse button went down in a dialog's window; `data` is the dialog.
    pub const DIALOG: i32 = 7;
    /// A menu-column group was chosen without a control; `first` the group, `data` its record.
    pub const GROUP: i32 = 8;
    /// A control of a group was chosen; `first` the group, `second` the control.
    pub const CONTROL: i32 = 9;
    /// The time line was dragged.
    pub const TIMELINE: i32 = 0xd;
    /// A button of the tablet's puck (devices 105 to 108); `second` the device.
    pub const PUCK: i32 = 0x10;
    /// Queue device 539 (IRIS GL WINFREEZE: a window was iconified).
    pub const WINFREEZE: i32 = 0x16;
    /// Queue device 540 (WINTHAW).
    pub const WINTHAW: i32 = 0x17;
    /// Handled where it was found (current-frame box, status box).
    pub const HANDLED: i32 = 0x19;
    /// The escape key went up; `second` is 0x1f, `x` the pointer's x.
    pub const ESCAPE: i32 = 0x1a;
    /// Queue device 528 (REDRAW).
    pub const REDRAW: i32 = 0x1b;
    /// Queue device 542 (WINQUIT).
    pub const WINQUIT: i32 = 0x1c;
    /// Swift-key learn mode: the click named an action; `data` is its string.
    pub const LEARN: i32 = 0x22;
}

record! {
    /// A canvas: one screen of the program (a module's menu columns, the title bar and the
    /// views). `SI_template` is one, and `setTemplate` copies fields at equal offsets from
    /// the first canvas of the network into it, which is how the layout is known to be the
    /// same. Owned by `server.c` (not ported); only what `event.c` and `window.c` use is named.
    pub struct Canvas = "SI_Canvas", size 0x7c {
        0x00 u_000: [u8; 4], "the module's number (finding 0019); not used by these files";
        0x04 name: Addr, "the canvas's name, as shown in the title bar for a module";
        0x08 title_left: i16, "the title (module) bar: left";
        0x0a title_right: i16, "right";
        0x0c title_bottom: i16, "bottom";
        0x0e title_top: i16, "top";
        0x10 w_010: i16, "copied by setTemplate";
        0x12 u_012: [u8; 10], "not used by these files";
        0x1c d_01c: u32, "copied by setTemplate";
        0x20 w_020: [i16; 4], "copied by setTemplate";
        0x28 active: [i16; 3], "three counts; a sum above 1 makes a click select as the right button does";
        0x2e left_left: i16, "the left menu column: left";
        0x30 left_right: i16, "right";
        0x32 left_bottom: i16, "bottom";
        0x34 left_top: i16, "top";
        0x36 n_left: i16, "number of groups in the left column";
        0x38 left_groups: P<Group>, "the left column's groups";
        0x3c right_left: i16, "the right menu column: left";
        0x3e right_right: i16, "right";
        0x40 right_bottom: i16, "bottom";
        0x42 right_top: i16, "top";
        0x44 n_right: i16, "number of groups in the right column";
        0x46 u_046: [u8; 2], "padding";
        0x48 right_groups: P<Group>, "the right column's groups";
        0x4c saved_left: i16, "in the template: the rectangle a zoomed view window had before, left";
        0x4e saved_bottom: i16, "bottom";
        0x50 saved_right: i16, "right";
        0x52 saved_top: i16, "top";
        0x54 n_views: i16, "number of view windows";
        0x56 views_left: i16, "the area of the views: left";
        0x58 views_right: i16, "right";
        0x5a views_bottom: i16, "bottom";
        0x5c views_top: i16, "top";
        0x5e u_05e: [u8; 2], "padding";
        0x60 views: P<Grafport>, "the view windows, an array";
        0x64 w_064: [i16; 4], "copied by setTemplate";
        0x6c u_06c: [u8; 8], "not used by these files";
        0x74 winid: i16, "the IRIS GL window that has the input focus (-1 at start)";
        0x76 u_076: [u8; 6], "not used by these files";
    }
}

record! {
    /// A group of a menu column, 0x14 bytes (the group arrays are walked in steps of ten
    /// 16-bit words). Owned by `control.c` (not ported).
    pub struct Group = "SI_Group", size 0x14 {
        0x00 id: i16, "identifier; 1, 2, 3 are the scaling, rotation and translation boxes, -100 the Exit button";
        0x02 state: i16, "0 off, 2 on, 99 and 999 while a swift-key mode holds it";
        0x04 u_004: [u8; 10], "not used by these files";
        0x0e n_controls: i16, "number of controls";
        0x10 controls: P<Control>, "the controls";
    }
}

record! {
    /// A control of a group. Owned by `control.c` (not ported); its size is not established
    /// here, only the fields these files read.
    pub struct Control = "SI_Control", size 0x20 {
        0x00 id: i16, "identifier within the group; 0, 1, 2 are x, y, z in the transformation boxes";
        0x02 state: i16, "non-zero when active";
        0x04 kind: i16, "3 and 4 are the kinds that open a menu";
        0x06 x: i16, "left edge";
        0x08 u_008: [u8; 2], "not used by these files";
        0x0a y: i16, "lower edge";
        0x0c u_00c: [u8; 4], "not used by these files";
        0x10 name: Addr, "the label";
        0x14 u_014: [u8; 8], "not used by these files";
        0x1c menu: Addr, "the menu a control of kind 3 or 4 opens";
    }
}

record! {
    /// A view's window, 0x2b8 bytes (the array the canvas points to is allocated as count ×
    /// 0x2b8 by `windowReadFile` and walked in steps of 0x2b8 everywhere).
    pub struct Grafport = "__SI_Grafport", size 0x2b8 {
        0x000 name: Addr, "the view's name from the layout file";
        0x004 id: i16, "1000 to 1003 in the standard layout: lower left, upper left, lower right, upper right";
        0x006 visible: i16, "non-zero when the window is shown";
        0x008 hilite: i16, "non-zero for the selected window";
        0x00a enabled: i16, "the fourth number of the layout file; a window where it is zero reacts only to its zoom box and its title";
        0x00c zoomed: i16, "set while the window is enlarged";
        0x00e u_00e: [u8; 2], "padding";
        0x010 expand: i32, "0 not expanded, 1 across, 2 upwards, both for a full-size window";
        0x014 left: i16, "the window's rectangle on the screen: left";
        0x016 right: i16, "right";
        0x018 bottom: i16, "bottom";
        0x01a top: i16, "top";
        0x01c view_left: i16, "the drawing area inside the border, the title bar and the rulers: left";
        0x01e view_right: i16, "right";
        0x020 view_bottom: i16, "bottom";
        0x022 view_top: i16, "top";
        0x024 view2: [i16; 4], "a second copy of the drawing area, in the order left, right, bottom, top";
        0x02c camera: Addr, "the camera in use (`SI_Camera`; +4 its type 2000.., +0x64 its aspect ratio)";
        0x030 cameras: Addr, "one 0x8c-byte record per possible camera (`SI_SoftWindow`)";
        0x034 d_034: Addr, "set by SI_setCurrentCam";
        0x038 u_038: [u8; 4], "not used by these files";
        0x03c d_03c: i32, "zero at start";
        0x040 d_040: Addr, "zero at start";
        0x044 d_044: u32, "zero at start";
        0x048 ruler_v: Addr, "the vertical ruler (`SI_Ruler`; +0x10 its width, +0x20 whether shown)";
        0x04c ruler_h: Addr, "the horizontal ruler";
        0x050 d_050: i32, "-1 at start";
        0x054 d_054: i32, "2 at start";
        0x058 menu_view: Addr, "the pop-up of view types (Front, Top, Right, Perspective, …)";
        0x05c view_item: i16, "an entry of the view-type menu the middle button on the title returns to; 6 at start";
        0x05e u_05e: [u8; 2], "padding";
        0x060 menu_display: Addr, "the pop-up of display modes (Wire frame, Shade, …)";
        0x064 display: i32, "the display mode, 0 wire frame";
        0x068 d_068: i32, "1 at start";
        0x06c w_06c: i16, "zero at start";
        0x06e u_06e: [u8; 2], "padding";
        0x070 roto: [u32; 24], "the rotoscopy settings, copied as a block into each camera record";
        0x0d0 u_0d0: [u8; 0x18c], "ghost settings and other blocks these files do not touch";
        0x25c fast: i16, "takes part in fast playback; zero at start";
        0x25e u_25e: [u8; 14], "fast-playback bookkeeping";
        0x26c d_26c: Addr, "zero at start";
        0x270 u_270: [u8; 4], "not used by these files";
        0x274 d_274: i32, "1 at start (depth cue)";
        0x278 d_278: i32, "-1 at start";
        0x27c d_27c: i32, "-1 at start";
        0x280 u_280: [u8; 8], "two floats of the depth cue";
        0x288 w_288: [i16; 3], "zero at start";
        0x28e u_28e: [u8; 2], "padding";
        0x290 d_290: u32, "zero at start";
        0x294 memo: Addr, "the stored camera positions, set by SI_setCurrentCam";
        0x298 ctrl: Addr, "the camera control buttons (`SI_Ctrlcam`); its first 16-bit word is non-zero while they are shown";
        0x29c menu_schematic: Addr, "the pop-up of schematic modes (Light, Model, Motion, Matter)";
        0x2a0 props: Addr, "graphic properties, set by SI_setCurrentCam";
        0x2a4 u_2a4: [u8; 4], "not used by these files";
        0x2a8 spreadsheet: Addr, "the spreadsheet shown in this window, or null";
        0x2ac u_2ac: [u8; 8], "spreadsheet set-up";
        0x2b4 menu_queries: Addr, "the pop-up of spreadsheet queries";
    }
}

record! {
    /// `SimpleListPtrC` of `drawWindow.c`: a list of pointers (`operator new(0xc)`).
    pub struct SimpleList = "SimpleListPtrC", size 0xc {
        0x00 count: i32, "number of places";
        0x04 step: i32, "by how many places a full list grows";
        0x08 items: Addr, "the places: an array of pointers, null for a free one";
    }
}

record! {
    /// `ShaderWindowC` of `drawWindow.c`: one extra IRIS GL window (`operator new(0x60)`).
    pub struct ShaderWindow = "ShaderWindowC", size 0x60 {
        0x00 name: [u8; 0x40], "the window's title, cut to 64 bytes (no terminating zero when it is that long)";
        0x40 x: i32, "left edge on the screen";
        0x44 y: i32, "lower edge";
        0x48 width: i32, "width as IRIS GL reported it after opening";
        0x4c height: i32, "height";
        0x50 data: Addr, "a word of the owner's, handed to the callback";
        0x54 d_054: Addr, "a block freed with the window; null when made";
        0x58 winid: i32, "the IRIS GL window";
        0x5c callback: Addr, "function called with an event, the IRIS GL window and the data word";
    }
}

record! {
    /// One entry of the table of a view window's parts (`gadgetInfo`, 55 entries indexed by
    /// part number), filled by `initGadget` from the static table `gadgetTitles`.
    pub struct Gadget = "gadgetInfo[]", size 0x1c {
        0x00 camera: i32, "the camera type whose title bar shows the gadget, -1 for the parts every view has";
        0x04 label: Addr, "the gadget's text";
        0x08 label2: Addr, "a second text drawn after it (a glyph), or an empty string";
        0x0c offset: i32, "where the gadget starts, in pixels from the window's left edge";
        0x10 width: i32, "its width in pixels";
        0x14 d_014: i32, "15 or 20 for the title-bar gadgets; not read by the functions ported";
        0x18 handler: Addr, "the function called when the part is clicked";
    }
}

record! {
    /// What a gadget's handler is called with (a local of `si_viewSelectHandle`).
    pub struct Callback = "callbackStruct", size 0x10 {
        0x00 event: P<Event>, "the event to fill in";
        0x04 x: i16, "the click's x";
        0x06 y: i16, "the click's y";
        0x08 window: P<Grafport>, "the view window";
        0x0c part: i32, "the part that was hit";
    }
}

/// Offsets into records of other parts of the program, of which these files read single
/// fields (the records themselves are not established here).
pub mod off {
    /// `SI_Camera`: the view type, 16 bits (2000 front … 2003 perspective, 2007, 2008, 2009).
    pub const CAMERA_TYPE: u32 = 0x04;
    /// `SI_Camera`: non-zero when the view shows a margin around its drawing area, 16 bits.
    pub const CAMERA_0C: u32 = 0x0c;
    /// `SI_Camera`: tested for the perspective view's "1:1" gadget, 16 bits.
    pub const CAMERA_44: u32 = 0x44;
    /// `SI_Camera`: the aspect ratio, a float.
    pub const CAMERA_ASPECT: u32 = 0x64;
    /// `SI_Camera`: three floats; the first is the centre of a front, top, function-curve,
    /// schematic or dope-sheet view across, the second upwards, the third serves the right
    /// view across and the top view upwards (`SI_windowLimitsHoriz`, `SI_windowLimitsVert`).
    pub const CAMERA_10: u32 = 0x10;
    pub const CAMERA_14: u32 = 0x14;
    pub const CAMERA_18: u32 = 0x18;
    /// `SI_Camera`: the field of view (perspective) or the height shown (other views), a float.
    pub const CAMERA_FOV: u32 = 0x40;
    /// `SI_Camera`: a float the rotoscope field of view is divided by.
    pub const CAMERA_50: u32 = 0x50;
    /// `SI_Camera`: a float the aspect ratio is rebuilt from after a zoomed rotoscope view
    /// goes back (the picture's own ratio, by its use).
    pub const CAMERA_68: u32 = 0x68;
    /// `__SI_Grafport`, in the rotoscopy block: non-zero when a picture is in use, 16 bits.
    pub const GRAF_ROTO_74: u32 = 0x74;
    /// `__SI_Grafport`, rotoscopy: the scale handed to `SI_dovPictScale`, a float.
    pub const GRAF_ROTO_SCALE: u32 = 0x94;
    /// `__SI_Grafport`, rotoscopy: a mode, 16 bits (2: the view's proportions are kept).
    pub const GRAF_ROTO_A0: u32 = 0xa0;
    /// `__SI_Grafport`, rotoscopy: the index of the picture in use, 32 bits.
    pub const GRAF_ROTO_INDEX: u32 = 0xa8;
    /// `__SI_Grafport`, rotoscopy: the pictures, 0x14 bytes each: an `SI_PictureRGBA` at +4
    /// (its 16-bit word at +6 of the entry is the height) and a pointer at +0x10 that is
    /// null while the picture is not loaded.
    pub const GRAF_ROTO_PICTURES: u32 = 0xac;
    /// `__SI_Grafport`, rotoscopy: the picture buffer (`SI_PictureBuf`) of the window.
    pub const GRAF_ROTO_BUF: u32 = 0xb0;
    /// `__SI_Grafport`, rotoscopy: a word that must be zero for the picture to be scaled.
    pub const GRAF_ROTO_B8: u32 = 0xb8;
    /// `SI_Ruler`: the width in pixels, 32 bits.
    pub const RULER_WIDTH: u32 = 0x10;
    /// `SI_Ruler`: non-zero when shown, 16 bits.
    pub const RULER_ACTIVE: u32 = 0x20;
    /// `__SI_Dialog`: the dialog's IRIS GL window, 32 bits.
    pub const DIALOG_WINID: u32 = 0x1c;
    /// `__SI_Dialog`: left, right, bottom, top, 16 bits each from here.
    pub const DIALOG_RECT: u32 = 0x04;
    /// `__SI_Dialog`: a count, 16 bits (number of items of the first kind).
    pub const DIALOG_N_0E: u32 = 0x0e;
    /// `__SI_Dialog`: a count, 16 bits.
    pub const DIALOG_N_10: u32 = 0x10;
}

/// Camera types (`SI_Camera` + 4) that these files tell apart.
pub mod cam {
    pub const FRONT: i16 = 2000;
    pub const TOP: i16 = 2001;
    pub const RIGHT: i16 = 2002;
    pub const PERSPECTIVE: i16 = 2003;
    /// The function-curve view.
    pub const FCURVE: i16 = 2004;
    /// The schematic view.
    pub const SCHEMATIC: i16 = 2007;
    /// The dope sheet.
    pub const DOPE_SHEET: i16 = 2008;
    /// The free orthogonal view.
    pub const ORTHO: i16 = 2009;
    /// The spreadsheet.
    pub const SPREADSHEET: i16 = 2010;
}

// ---- scrollbar.c (package Controls)
record! {
    /// A scroll bar made by `SI_scrollbarCreate` (`scrollbar.c`): horizontal or vertical, with
    /// a slider and two arrow boxes. Layout read from the code; the names are ours.
    pub struct Scrollbar = "__SI_Scrollbar", size 0x40 {
        0x00 grafport: Addr, "the window it is drawn in (its +6 word is zero while that is hidden)";
        0x04 x: i32, "left edge";
        0x08 y: i32, "top edge";
        0x0c length: i32, "length along its direction";
        0x10 vertical: i16, "non-zero for a vertical bar";
        0x12 u_012: [u8; 2], "padding, never written";
        0x14 range: u32, "the number of units the bar stands for";
        0x18 visible: u32, "the units that are in view (the slider's size)";
        0x1c pos: u32, "the first unit in view";
        0x20 page: u32, "how far a page move goes";
        0x24 inc_left: u32, "the move of the first mouse button (device 0x67)";
        0x28 inc_middle: u32, "the move of the second (0x66)";
        0x2c inc_right: u32, "the move of the third (0x65)";
        0x30 delay_first: i16, "ticks before the first repeat";
        0x32 delay_next: i16, "ticks between repeats";
        0x34 user: Addr, "handed to the callback";
        0x38 callback: Addr, "called with the bar, the user data and the new position";
        0x3c part: i32, "the part last pressed (0 none, 1 to 4 arrows, 5 slider, 6 and 7 the shaft)";
    }
}
