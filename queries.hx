// Query definitions that use the schema
QUERY getUserPosts(userId: String) =>
    user <- N<User>({name: userId})
    posts <- user::Out<Wrote>
    RETURN posts::{title, content, created_at}

QUERY createUser(name: String, email: String, age: I32) =>
    newUser <- AddN<User>({name: name, email: email, age: age})
    RETURN newUser

QUERY getUserComments(userId: String) =>
    user <- N<User>({name: userId})
    comments <- user::Out<CommentedOn>::In<Replied>
    RETURN comments::{text, created_at}

// This should cause an error - invalid field
QUERY badQuery() =>
    user <- N<User>
    RETURN user::{invalid_field}

// This should cause an error - unknown edge type
QUERY anotherBadQuery() =>
    user <- N<User>
    posts <- user::Out<UnknownEdge>
    RETURN posts 