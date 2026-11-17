-- argv: action, query, id, title, body
on run argv
	set {action, theQuery, theId, theTitle, theBody} to {item 1 of argv, item 2 of argv, item 3 of argv, item 4 of argv, item 5 of argv}
	tell application "Notes"
		if action is "list" then
			set out to ""
			set n to 0
			set found to notes
			if theQuery is not "" then set found to (notes whose name contains theQuery)
			repeat with aNote in found
				set n to n + 1
				if n > 50 then exit repeat
				set out to out & my row({id of aNote, name of aNote, my isoDate(modification date of aNote)})
			end repeat
			return out
		end if
		if action is "create" then
			set lines to my splitText(theBody, linefeed)
			set html to "<div><h1>" & my htmlText(theTitle) & "</h1></div>"
			repeat with l in lines
				set html to html & "<div>" & my htmlText(l as text) & "</div>"
			end repeat
			set aNote to make new note with properties {body:html}
			return "created " & (id of aNote)
		end if
		set aNote to note id theId
		if action is "read" then return plaintext of aNote
		if action is "edit" then
			set html to ""
			if theTitle is not "" then set html to "<div><h1>" & my htmlText(theTitle) & "</h1></div>"
			repeat with l in my splitText(theBody, linefeed)
				set html to html & "<div>" & my htmlText(l as text) & "</div>"
			end repeat
			set body of aNote to html
			return "edited"
		end if
		if action is "delete" then
			delete aNote
			return "deleted"
		end if
	end tell
	error "unknown action " & action
end run

on htmlText(s)
	if s is "" then return "<br>"
	return my replaceText(my replaceText(my replaceText(s, "&", "&amp;"), "<", "&lt;"), ">", "&gt;")
end htmlText
