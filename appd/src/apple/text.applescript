-- Shared by the app scripts. Nothing here runs a string: text only ever arrives as `argv`.
on replaceText(s, find, repl)
	set saved to AppleScript's text item delimiters
	set AppleScript's text item delimiters to find
	set parts to text items of s
	set AppleScript's text item delimiters to repl
	set s to parts as text
	set AppleScript's text item delimiters to saved
	return s
end replaceText

on splitText(s, sep)
	set saved to AppleScript's text item delimiters
	set AppleScript's text item delimiters to sep
	set parts to text items of s
	set AppleScript's text item delimiters to saved
	return parts
end splitText

on trim(s)
	repeat while s starts with " " or s starts with tab
		if length of s = 1 then return ""
		set s to text 2 thru -1 of s
	end repeat
	repeat while s ends with " " or s ends with tab
		set s to text 1 thru -2 of s
	end repeat
	return s
end trim

-- "2026-03-01T09:00:00" (or just the day) as a date; built from its numbers, never parsed
-- by the locale.
on parseDate(s)
	try
		set d to current date
		set day of d to 1
		set year of d to (text 1 thru 4 of s) as integer
		set month of d to (text 6 thru 7 of s) as integer
		set day of d to (text 9 thru 10 of s) as integer
		if day of d is not (text 9 thru 10 of s) as integer then error
		set hh to 0
		set mm to 0
		set ss to 0
		if length of s ≥ 16 then
			set hh to (text 12 thru 13 of s) as integer
			set mm to (text 15 thru 16 of s) as integer
		end if
		if length of s ≥ 19 then set ss to (text 18 thru 19 of s) as integer
		set time of d to hh * hours + mm * minutes + ss
		return d
	on error
		error "`" & s & "` is not a date like 2026-03-01T09:00:00"
	end try
end parseDate

on pad(n)
	return text -2 thru -1 of ("0" & n)
end pad

on isoDate(d)
	return (year of d as text) & "-" & pad(month of d as integer) & "-" & pad(day of d) & "T" & pad(hours of d) & ":" & pad(minutes of d) & ":" & pad(seconds of d)
end isoDate

on clip(s, n)
	if length of s > n then return (text 1 thru n of s) & "…"
	return s
end clip

-- One reply line: the fields joined by tabs, with no tab or line break inside a field.
on row(fields)
	set out to ""
	repeat with f in fields
		set t to ""
		if f is not missing value then set t to my replaceText(my replaceText(my replaceText(f as text, tab, " "), return, " "), linefeed, " ")
		set out to out & t & tab
	end repeat
	return (text 1 thru -2 of out) & linefeed
end row
