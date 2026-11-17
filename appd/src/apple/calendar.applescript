-- argv: action, id, calendar, title, start, end, location, notes
on run argv
	set {action, theId, theCalendar, theTitle, theStart, theEnd, theLocation, theNotes} to {item 1 of argv, item 2 of argv, item 3 of argv, item 4 of argv, item 5 of argv, item 6 of argv, item 7 of argv, item 8 of argv}
	tell application "Calendar"
		if action is "list" then
			set s to current date
			if theStart is not "" then set s to my parseDate(theStart)
			set e to s + 7 * days
			if theEnd is not "" then set e to my parseDate(theEnd)
			set out to ""
			set n to 0
			repeat with c in calendars
				if theCalendar is "" or name of c is theCalendar then
					repeat with ev in (events of c whose start date ≥ s and start date < e)
						set n to n + 1
						if n > 100 then exit repeat
						set out to out & my row({uid of ev, name of c, summary of ev, my isoDate(start date of ev), my isoDate(end date of ev), location of ev})
					end repeat
				end if
			end repeat
			return out
		end if
		if action is "create" then
			if theTitle is "" or theStart is "" then error "`title` and `start` are needed"
			set s to my parseDate(theStart)
			set e to s + 1 * hours
			if theEnd is not "" then set e to my parseDate(theEnd)
			set props to {summary:theTitle, start date:s, end date:e}
			if theLocation is not "" then set props to props & {location:theLocation}
			if theNotes is not "" then set props to props & {description:theNotes}
			if theCalendar is "" then
				set target to first calendar whose writable is true
			else
				set target to calendar theCalendar
			end if
			set ev to make new event at end of events of target with properties props
			return "created " & (uid of ev)
		end if
		set ev to missing value
		repeat with c in calendars
			set found to (events of c whose uid is theId)
			if (count of found) > 0 then
				set ev to item 1 of found
				exit repeat
			end if
		end repeat
		if ev is missing value then error "no event with that id"
		if action is "edit" then
			if theTitle is not "" then set summary of ev to theTitle
			if theStart is not "" then set start date of ev to my parseDate(theStart)
			if theEnd is not "" then set end date of ev to my parseDate(theEnd)
			if theLocation is not "" then set location of ev to theLocation
			if theNotes is not "" then set description of ev to theNotes
			return "edited"
		end if
		if action is "delete" then
			delete ev
			return "deleted"
		end if
	end tell
	error "unknown action " & action
end run
