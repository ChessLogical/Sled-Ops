/1 and /2 and /3 are a mess   /4 is a little better 

/5 experimental /6 still experimental

7--showing promise
8-- yikes i think it introduced some bugs


9--- getting a lil crazy, main.rs makes html template strings now for easy debugging cuz its getting complicated


10---render_main_page_html: Generates the main page HTML with the list of posts and pagination links.
render_post_view_html: Generates the HTML for viewing a specific post along with its replies.
render_post_html: Generates the HTML for an individual post.
render_reply_html: Generates the HTML for an individual reply 
Code is agnostic (makes its own directories so run it in any directory)
latest reply brings post to top in main page, 
At this point requires intense testing to find and eliminate bugs. that will take a few days
but then after that 80+ percent of app should be done and tested. 

11/ refined to get warnings out, still untested and also a critical time in the dev because from here it can fork out to 
be very different things. 

12// could have jumped from 11 many different ways. I went with askama template engine. Why? Compile-Time Safety: Askama templates are compiled into Rust code at build time, which ensures that any errors in the templates are caught early. This can lead to fewer runtime errors and more reliable code. Performance: Because Askama templates are compiled into Rust code, they can be highly optimized. The overhead of parsing and interpreting templates at runtime is eliminated, leading to faster execution. Just 1 of many possibilities, for now i will stick with it and dev from here. At any point anyone can go back to 11 and jump in other directions. . 

13- slight improvments. suitable for being called version 0001
