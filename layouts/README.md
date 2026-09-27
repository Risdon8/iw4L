# Map layouts (mod)

A layout is a JSON file applied on top of a stock map when it loads. It adds
collision shapes dressed with stretched props from that map, replaces the spawn
points, resets players who fall out, and can switch surf on.

## Play one

In the console:

```
layout highrise_playground     load the layout and its base map
layout                         show the active layout and the ones available
layout reload                  re-read the file and reload the map
layout off                     stock map on the next load
```

Or start with it: `IW4L_LAYOUT=highrise_playground iw4l.exe map mp_highrise`
(`IW4L_LAYOUT` also works from `.env`).

## highrise_playground

A floating course in the street canyon south of the Highrise towers:

1. **Surf** — walk off the start deck onto ramp 1 and hold **D** only (no W).
   Stay on the same side for ramp 2.
2. **Mid deck**, then the **hop line**: hold space, autobhop is on.
3. **Wall-run corridor** — two long walls with a floor for now; the floor goes
   when wall-running exists.
4. **Portal** on the end deck sends you back to the start. Falling below
   z 1500 does too.

## Writing one

```json
{
  "name": "my_layout",
  "base_map": "mp_highrise",
  "movement": { "surf": true },
  "spawns": [{ "origin": [x, y, z], "yaw": 0 }],
  "reset": { "below_z": 1500, "volumes": [{ "min": [..], "max": [..] }] },
  "ignore_player_clip": true,
  "default_model": "ch_crate64x64",
  "shapes": [
    { "type": "box",  "center": [x, y, z], "size": [sx, sy, sz], "angles": [pitch, yaw, roll] },
    { "type": "ramp", "center": [x, y, z], "length": 1400, "width": 320, "drop": 350, "yaw": 0 }
  ]
}
```

* **box** — oriented box; `center` is its middle.
* **ramp** — surf wedge lying along `yaw`; `center` is the middle of its base.
  Faces are 60° unless `angle` (degrees) or `height` is given; `drop` tilts it
  so the far end is lower and gravity carries you along it (about 15° works).
* Any shape: `"model"` picks the prop that is stretched over it (any model in
  the base map; the load log lists them as `static model mesh:`), and
  `"visible": false` leaves it as invisible collision.
* `ignore_player_clip` lets players into the map's out-of-bounds air, which
  is filled with invisible clip.
* Units are game units (a player is 70 tall, 30 wide; walking is 190 u/s).

Finding coordinates: `showpos` prints where you stand, `tp x y z yaw pitch`
moves you (add `&` if you will fall), and `clipprobe [dist]` traces forward and
says whether the map or the layout is in the way. The load log prints where the
stock spawns sit (`stock spawns within ...`).
