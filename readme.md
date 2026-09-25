D:\develops\git\github\rust\axum-web-app
参考这个rust工程

php http server项目
添加git submodule
[submodule "third_party/shopex-ecshop"]
	path = third_party/shopex-ecshop
	url = https://gitee.com/softtomorrow/ecshop

git init
git remote add codeup mydada@mydada-cn-hangzhou.devops.aliyuncs.com:codeup/edidada/rust_ecshop.git
git remote add git@github.com:edidada/rust_ecshop.git
搭建骨架测试通过之后 编写github action yml，支持三个主流os

git push origin --all
git push codeup --all
然后按照url，编码，git add commit push两个远程仓库
注意不要编译测试，只要之前骨架编译通过就行
一个url git add commit push两个远程仓库一次
我稍后集中编译测试