-- argv: action, id, list, title, body, due
on run argv
	set {action, theId, theList, theTitle, theBody, theDue} to {item 1 of argv, item 2 of argv, item 3 of argv, item 4 of argv, item 5 of argv, item 6 of argv}
	tell application "Reminders"
		if action is "list" then
			set out to ""
			set n to 0
			repeat with aList in lists
				if theList is "" or name of aList is theList then
					repeat with r in (reminders of aList whose completed is false)
						set n to n + 1
						if n > 100 then exit repeat
						set due to ""
						if due date of r is not missing value then set due to my isoDate(due date of r)
						set out to out & my row({id of r, name of aList, name of r, due, body of r})
					end repeat
				end if
			end repeat
			return out
		end if
		if action is "create" then
			if theTitle is "" then error "`title` is empty"
			set props to {name:theTitle}
			if theBody is not "" then set props to props & {body:theBody}
			if theDue is not "" then set props to props & {due date:my parseDate(theDue)}
			if theList is "" then
				set target to default list
			else
				set target to list theList
			end if
			set r to make new reminder at end of reminders of target with properties props
			return "created " & (id of r)
		end if
		set r to reminder id theId
		if action is "edit" then
			if theTitle is not "" then set name of r to theTitle
			if theBody is not "" then set body of r to theBody
			if theDue is not "" then set due date of r to my parseDate(theDue)
			return "edited"
		end if
		if action is "complete" then
			set completed of r to true
			return "completed"
		end if
		if action is "delete" then
			delete r
			return "deleted"
		end if
	end tell
	error "unknown action " & action
end run
