-- ISS APRS decoder: the core demodulates the 1200 Bd AFSK and finds the AX.25 frames,
-- this script only says what to show. Every frame arrives in on_frame(frame):
--   frame.src   sender call sign, e.g. "HA5XYZ-7"
--   frame.dst   destination, e.g. "APRS"
--   frame.path  digipeaters, e.g. {"RS0ISS*"}
--   frame.info  the message text

function on_frame(frame)
  show.text(frame.src .. " > " .. frame.dst .. ": " .. frame.info)

  -- APRS positions start with ! or = (or / and @ with a time stamp)
  local lat, lon = aprs.position(frame.info)
  if lat then
    show.map(lat, lon, frame.src)
  end
end
