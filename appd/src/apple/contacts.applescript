-- argv: action, query, id, first, last, org, email, phone, note
on run argv
	set {action, theQuery, theId, theFirst, theLast, theOrg, theEmail, thePhone, theNote} to {item 1 of argv, item 2 of argv, item 3 of argv, item 4 of argv, item 5 of argv, item 6 of argv, item 7 of argv, item 8 of argv, item 9 of argv}
	tell application "Contacts"
		if action is "search" then
			set out to ""
			set n to 0
			set found to people
			if theQuery is not "" then set found to (people whose name contains theQuery)
			repeat with p in found
				set n to n + 1
				if n > 50 then exit repeat
				set mails to ""
				repeat with m in emails of p
					set mails to mails & (value of m) & " "
				end repeat
				set phones to ""
				repeat with t in phones of p
					set phones to phones & (value of t) & " "
				end repeat
				set out to out & my row({id of p, name of p, organization of p, mails, phones})
			end repeat
			return out
		end if
		if action is "create" then
			set p to make new person with properties {first name:theFirst, last name:theLast, organization:theOrg, note:theNote}
			my addDetails(p, theEmail, thePhone)
			save
			return "created " & (id of p)
		end if
		set p to person id theId
		if action is "edit" then
			if theFirst is not "" then set first name of p to theFirst
			if theLast is not "" then set last name of p to theLast
			if theOrg is not "" then set organization of p to theOrg
			if theNote is not "" then set note of p to theNote
			my addDetails(p, theEmail, thePhone)
			save
			return "edited"
		end if
		if action is "delete" then
			delete p
			save
			return "deleted"
		end if
	end tell
	error "unknown action " & action
end run

on addDetails(p, theEmail, thePhone)
	tell application "Contacts"
		if theEmail is not "" then make new email at end of emails of p with properties {label:"home", value:theEmail}
		if thePhone is not "" then make new phone at end of phones of p with properties {label:"mobile", value:thePhone}
	end tell
end addDetails
