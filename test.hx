// Test HelixQL file
N::User {
    name: String,
    age: I32,
    email: String
}

N::Post {
    title: String,
    content: String,
    created_at: Date
}

E::Wrote {
    From: User,
    To: Post,
    Properties: {
        timestamp: Date
    }
}

QUERY getUserPosts(userId: String) =>
    user <- N<User>({id: userId})
    posts <- user::Out<Wrote>
    RETURN posts::{title, content}

QUERY createUser(name: String, email: String) =>
    newUser <- AddN<User>({name: name, email: email, age: 25})
    RETURN newUser 